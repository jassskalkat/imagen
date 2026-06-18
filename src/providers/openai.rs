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
    /// Create a new OpenAIProvider from the application config.
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
                .unwrap(),
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
}

#[async_trait]
impl ImageProvider for OpenAIProvider {
    #[instrument(skip(self, request), fields(provider = "openai"))]
    async fn generate(&self, request: &GenerateRequest) -> Result<ProviderResponse> {
        let model = request.model.as_deref().unwrap_or(&self.default_model);

        let size = request
            .size
            .as_ref()
            .map(|s| s.as_str())
            .unwrap_or("1024x1024");

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
            "response_format": "b64_json"
        });

        if let Some(ref style) = request.style {
            body["style"] = serde_json::to_value(style).unwrap_or(json!("vivid"));
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
                let request_builder = client
                    .post(OPENAI_GENERATIONS_URL)
                    .header("Content-Type", "application/json")
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .json(body);

                let request_builder = if let Some(ref org_id) = self.org_id {
                    request_builder.header("OpenAI-Organization", org_id)
                } else {
                    request_builder
                };

                let response = request_builder.send().await?;

                let status = response.status();
                let response_body: serde_json::Value = response.json().await?;

                if !status.is_success() {
                    let error_msg = response_body["error"]["message"]
                        .as_str()
                        .unwrap_or("Unknown OpenAI error");
                    return Err(match status.as_u16() {
                        401 | 403 => ImagenError::ProviderAuth(error_msg.to_string()),
                        429 => ImagenError::RateLimit(error_msg.to_string()),
                        code => ImagenError::ProviderError {
                            message: format!("OpenAI API error ({}): {}", status, error_msg),
                            status_code: Some(code),
                        },
                    });
                }

                self.parse_response(response_body, model_owned)
            }
        })
        .await
    }

    #[instrument(skip(self, request), fields(provider = "openai"))]
    async fn edit(&self, request: &EditRequest) -> Result<ProviderResponse> {
        let model = request.model.as_deref().unwrap_or(&self.default_model);

        // Read file bytes outside the retry loop
        let image_bytes = if let Some(image_path) = request.image_paths.first() {
            let bytes = tokio::fs::read(image_path).await.map_err(|e| {
                ImagenError::FileError(format!("Failed to read image {}: {}", image_path, e))
            })?;
            let filename = std::path::Path::new(image_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("image.png")
                .to_string();
            Some((bytes, filename))
        } else {
            None
        };

        let mask_bytes = if let Some(ref mask_path) = request.mask_path {
            let bytes = tokio::fs::read(mask_path).await.map_err(|e| {
                ImagenError::FileError(format!("Failed to read mask {}: {}", mask_path, e))
            })?;
            let filename = std::path::Path::new(mask_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("mask.png")
                .to_string();
            Some((bytes, filename))
        } else {
            None
        };

        debug!(url = OPENAI_EDITS_URL, "Sending edit request to OpenAI");

        let model_owned = model.to_string();
        let size = request.size.clone();
        let prompt = request.prompt.clone();
        let n = request.n.unwrap_or(1);

        with_retry(|| {
            let client = &self.client;
            let model_owned = &model_owned;
            let image_bytes = &image_bytes;
            let mask_bytes = &mask_bytes;
            let size = &size;
            let prompt = &prompt;
            async move {
                let mut form = multipart::Form::new()
                    .text("model", model_owned.to_string())
                    .text("prompt", prompt.clone())
                    .text("n", n.to_string())
                    .text("response_format", "b64_json".to_string());

                if let Some(ref size) = size {
                    form = form.text("size", size.as_str().to_string());
                }

                if let Some((ref bytes, ref filename)) = *image_bytes {
                    let part = multipart::Part::bytes(bytes.clone()).file_name(filename.clone());
                    form = form.part("image", part);
                }

                if let Some((ref bytes, ref filename)) = *mask_bytes {
                    let part = multipart::Part::bytes(bytes.clone()).file_name(filename.clone());
                    form = form.part("mask", part);
                }

                let request_builder = client
                    .post(OPENAI_EDITS_URL)
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .multipart(form);

                let request_builder = if let Some(ref org_id) = self.org_id {
                    request_builder.header("OpenAI-Organization", org_id)
                } else {
                    request_builder
                };

                let response = request_builder.send().await?;

                let status = response.status();
                let response_body: serde_json::Value = response.json().await?;

                if !status.is_success() {
                    let error_msg = response_body["error"]["message"]
                        .as_str()
                        .unwrap_or("Unknown OpenAI error");
                    return Err(match status.as_u16() {
                        401 | 403 => ImagenError::ProviderAuth(error_msg.to_string()),
                        429 => ImagenError::RateLimit(error_msg.to_string()),
                        code => ImagenError::ProviderError {
                            message: format!("OpenAI API error ({}): {}", status, error_msg),
                            status_code: Some(code),
                        },
                    });
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
                max_images: 4,
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
        let config = test_config();
        let provider = OpenAIProvider::new(&config).unwrap();
        assert_eq!(provider.api_key, "sk-test-key-12345");
        assert_eq!(provider.org_id, Some("org-test".to_string()));
    }

    #[test]
    fn test_openai_provider_missing_key() {
        let mut config = test_config();
        config.openai_api_key = None;
        let result = OpenAIProvider::new(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_openai_get_models() {
        let config = test_config();
        let provider = OpenAIProvider::new(&config).unwrap();
        let models = provider.get_models();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "gpt-image-2");
        assert!(models[0].supports_editing);
        assert_eq!(models[1].id, "dall-e-3");
        assert!(!models[1].supports_editing);
    }

    #[test]
    fn test_parse_response() {
        let config = test_config();
        let provider = OpenAIProvider::new(&config).unwrap();

        let body = json!({
            "data": [
                {
                    "b64_json": "dGVzdGltYWdl",
                    "revised_prompt": "A colorful sunset"
                }
            ]
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
        let config = test_config();
        let provider = OpenAIProvider::new(&config).unwrap();
        assert_eq!(provider.provider_name(), "openai");
    }
}
