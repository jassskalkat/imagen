use async_trait::async_trait;
use reqwest::{multipart, Client};
use serde_json::json;
use std::time::Duration;
use tracing::{debug, instrument};

use crate::config::AppConfig;
use crate::error::{ImagenError, Result};
use crate::retry::with_retry;
use crate::types::{EditRequest, GenerateRequest, ImageData, ProviderResponse, UsageInfo};

use super::{ImageProvider, ModelInfo};

const OPENAI_GENERATIONS_URL: &str = "https://api.openai.com/v1/images/generations";
const OPENAI_EDITS_URL: &str = "https://api.openai.com/v1/images/edits";

/// OpenAI image generation provider.
pub struct OpenAIProvider {
    client: Client,
    api_key: String,
    org_id: Option<String>,
    default_model: String,
}

impl OpenAIProvider {
    pub fn new(config: &AppConfig) -> Result<Self> {
        let api_key = config
            .openai_api_key
            .as_deref()
            .ok_or_else(|| ImagenError::ConfigError("OpenAI API key not configured".into()))?
            .to_string();
        Ok(Self {
            client: Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .expect("failed to build reqwest client"),
            api_key,
            org_id: config.openai_org_id.clone(),
            default_model: config.default_model.clone(),
        })
    }

    fn parse_response(&self, body: serde_json::Value, model: &str) -> Result<ProviderResponse> {
        let data = body["data"]
            .as_array()
            .ok_or_else(|| ImagenError::ProviderError {
                message: "Missing 'data' in response".into(),
                status_code: None,
            })?;
        let images: Vec<ImageData> = data
            .iter()
            .map(|item| ImageData {
                b64_json: item["b64_json"].as_str().unwrap_or_default().to_string(),
                revised_prompt: item["revised_prompt"].as_str().map(|s| s.to_string()),
            })
            .collect();
        let usage = body.get("usage").map(|u| UsageInfo {
            input_tokens: u["input_tokens"].as_u64(),
            output_tokens: u["output_tokens"].as_u64(),
        });
        Ok(ProviderResponse {
            images,
            model: model.to_string(),
            usage,
        })
    }

    fn map_error(status: reqwest::StatusCode, body: &serde_json::Value) -> ImagenError {
        let msg = body["error"]["message"]
            .as_str()
            .unwrap_or("Unknown OpenAI error");
        match status.as_u16() {
            401 | 403 => ImagenError::ProviderAuth(msg.to_string()),
            429 => ImagenError::RateLimit(msg.to_string()),
            code => ImagenError::ProviderError {
                message: format!("OpenAI API error ({}): {}", status, msg),
                status_code: Some(code),
            },
        }
    }
}

fn background_str(bg: &crate::types::ImageBackground) -> &'static str {
    match bg {
        crate::types::ImageBackground::Opaque => "opaque",
        crate::types::ImageBackground::Auto => "auto",
    }
}

#[async_trait]
impl ImageProvider for OpenAIProvider {
    #[instrument(skip(self, request), fields(provider = "openai"))]
    async fn generate(&self, request: &GenerateRequest) -> Result<ProviderResponse> {
        let model = request.model.as_deref().unwrap_or(&self.default_model);
        let size = request
            .size
            .as_ref()
            .map(|s| s.as_str().to_string())
            .unwrap_or_else(|| "1024x1024".to_string());
        let quality = request
            .quality
            .as_ref()
            .map(|q| serde_json::to_value(q).unwrap_or(json!("standard")))
            .unwrap_or(json!("standard"));
        let n = request.n.unwrap_or(1);

        let mut body = json!({
            "model": model,
            "prompt": request.prompt,
            "n": n,
            "size": size,
            "quality": quality,
        });
        // response_format is only accepted by dall-e-2/dall-e-3; GPT image
        // models (gpt-image-1, gpt-image-2, ...) always return base64 images
        // and reject this parameter per the official API reference.
        if !model.contains("gpt-image") {
            body["response_format"] = json!("b64_json");
        }
        if let Some(ref style) = request.style {
            body["style"] = serde_json::to_value(style).unwrap_or(json!("vivid"));
        }
        if let Some(c) = request.output_compression {
            body["output_compression"] = json!(c);
        }
        if let Some(ref bg) = request.background {
            body["background"] = serde_json::to_value(bg).unwrap_or(json!("auto"));
        }
        if let Some(ref m) = request.moderation {
            body["moderation"] = json!(m);
        }
        if let Some(ref u) = request.user {
            body["user"] = json!(u);
        }

        debug!(
            url = OPENAI_GENERATIONS_URL,
            "Sending generation request to OpenAI"
        );
        let model_owned = model.to_string();
        with_retry(|| {
            let client = &self.client;
            let body = &body;
            let model_owned = &model_owned;
            async move {
                let rb = client
                    .post(OPENAI_GENERATIONS_URL)
                    .header("Content-Type", "application/json")
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .json(body);
                let rb = if let Some(ref org_id) = self.org_id {
                    rb.header("OpenAI-Organization", org_id)
                } else {
                    rb
                };
                let response = rb.send().await?;
                let status = response.status();
                let response_body: serde_json::Value = response.json().await?;
                if !status.is_success() {
                    return Err(Self::map_error(status, &response_body));
                }
                self.parse_response(response_body, model_owned)
            }
        })
        .await
    }

    #[instrument(skip(self, request), fields(provider = "openai"))]
    async fn edit(&self, request: &EditRequest) -> Result<ProviderResponse> {
        let model = request.model.as_deref().unwrap_or(&self.default_model);

        let mut all_image_bytes: Vec<(Vec<u8>, String)> = Vec::new();
        for image_path in &request.image_paths {
            let bytes = tokio::fs::read(image_path).await.map_err(|e| {
                ImagenError::FileError(format!("Failed to read image {}: {}", image_path, e))
            })?;
            let filename = std::path::Path::new(image_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("image.png")
                .to_string();
            all_image_bytes.push((bytes, filename));
        }

        let mask_bytes = if let Some(ref mask_path) = request.mask_path {
            let bytes = tokio::fs::read(mask_path).await.map_err(|e| {
                ImagenError::FileError(format!("Failed to read mask {}: {}", mask_path, e))
            })?;
            let fname = std::path::Path::new(mask_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("mask.png")
                .to_string();
            Some((bytes, fname))
        } else {
            None
        };

        debug!(url = OPENAI_EDITS_URL, "Sending edit request to OpenAI");
        let model_owned = model.to_string();
        let size = request.size.clone();
        let prompt = request.prompt.clone();
        let n = request.n.unwrap_or(1);
        let output_compression = request.output_compression;
        let background = request.background.clone();
        let moderation = request.moderation.clone();
        let user = request.user.clone();

        with_retry(|| {
            let client = &self.client;
            let model_owned = &model_owned;
            let all_image_bytes = &all_image_bytes;
            let mask_bytes = &mask_bytes;
            let size = &size;
            let prompt = &prompt;
            let background = &background;
            let moderation = &moderation;
            let user = &user;
            async move {
                let mut form = multipart::Form::new()
                    .text("model", model_owned.to_string())
                    .text("prompt", prompt.clone())
                    .text("n", n.to_string());
                // response_format is only accepted by dall-e-2; GPT image
                // models always return base64 images and reject this parameter.
                if !model_owned.contains("gpt-image") {
                    form = form.text("response_format", "b64_json".to_string());
                }
                if let Some(ref size) = size {
                    form = form.text("size", size.as_str().to_string());
                }
                if all_image_bytes.len() == 1 {
                    let (ref bytes, ref filename) = all_image_bytes[0];
                    let part = multipart::Part::bytes(bytes.clone()).file_name(filename.clone());
                    form = form.part("image", part);
                } else {
                    for (bytes, filename) in all_image_bytes.iter() {
                        let part =
                            multipart::Part::bytes(bytes.clone()).file_name(filename.clone());
                        form = form.part("image[]", part);
                    }
                }
                if let Some((ref bytes, ref filename)) = *mask_bytes {
                    let part = multipart::Part::bytes(bytes.clone()).file_name(filename.clone());
                    form = form.part("mask", part);
                }
                if let Some(c) = output_compression {
                    form = form.text("output_compression", c.to_string());
                }
                if let Some(ref bg) = *background {
                    form = form.text("background", background_str(bg).to_string());
                }
                if let Some(ref m) = *moderation {
                    form = form.text("moderation", m.clone());
                }
                if let Some(ref u) = *user {
                    form = form.text("user", u.clone());
                }
                let rb = client
                    .post(OPENAI_EDITS_URL)
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .multipart(form);
                let rb = if let Some(ref org_id) = self.org_id {
                    rb.header("OpenAI-Organization", org_id)
                } else {
                    rb
                };
                let response = rb.send().await?;
                let status = response.status();
                let response_body: serde_json::Value = response.json().await?;
                if !status.is_success() {
                    return Err(Self::map_error(status, &response_body));
                }
                self.parse_response(response_body, model_owned)
            }
        })
        .await
    }

    fn get_models(&self) -> Vec<ModelInfo> {
        vec![
            ModelInfo {
                id: "gpt-image-2".to_string(),
                name: "GPT Image 2".to_string(),
                supports_editing: true,
                max_images: 10,
            },
            ModelInfo {
                id: "dall-e-3".to_string(),
                name: "DALL-E 3".to_string(),
                supports_editing: false,
                max_images: 1,
            },
        ]
    }

    fn provider_name(&self) -> &'static str {
        "openai"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Provider;

    fn test_config() -> AppConfig {
        AppConfig {
            provider: Provider::OpenAI,
            azure_endpoint: None,
            azure_deployment_name: None,
            azure_api_key: None,
            azure_api_version: None,
            openai_api_key: Some("sk-test-key-12345".into()),
            openai_org_id: Some("org-test".into()),
            output_dir: "/tmp/test".into(),
            max_concurrent_jobs: 4,
            default_model: "gpt-image-2".into(),
        }
    }

    #[test]
    fn test_openai_provider_creation() {
        let provider = OpenAIProvider::new(&test_config()).unwrap();
        assert_eq!(provider.api_key, "sk-test-key-12345");
        assert_eq!(provider.org_id, Some("org-test".to_string()));
    }

    #[test]
    fn test_openai_provider_missing_key() {
        let mut config = test_config();
        config.openai_api_key = None;
        assert!(OpenAIProvider::new(&config).is_err());
    }

    #[test]
    fn test_openai_get_models() {
        let provider = OpenAIProvider::new(&test_config()).unwrap();
        let models = provider.get_models();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "gpt-image-2");
        assert!(models[0].supports_editing);
        assert_eq!(models[0].max_images, 10);
        assert_eq!(models[1].id, "dall-e-3");
        assert!(!models[1].supports_editing);
    }

    #[test]
    fn test_parse_response() {
        let provider = OpenAIProvider::new(&test_config()).unwrap();
        let body = json!({
            "data": [{
                "b64_json": "dGVzdGltYWdl",
                "revised_prompt": "A colorful sunset"
            }]
        });
        let response = provider.parse_response(body, "gpt-image-2").unwrap();
        assert_eq!(response.images.len(), 1);
        assert_eq!(response.images[0].b64_json, "dGVzdGltYWdl");
        assert_eq!(
            response.images[0].revised_prompt,
            Some("A colorful sunset".to_string())
        );
        assert_eq!(response.model, "gpt-image-2");
        assert!(response.usage.is_none());
    }

    #[test]
    fn test_provider_name() {
        let provider = OpenAIProvider::new(&test_config()).unwrap();
        assert_eq!(provider.provider_name(), "openai");
    }
}
