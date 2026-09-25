use async_trait::async_trait;
use reqwest::{multipart, Client};
use serde_json::json;
use std::time::Duration;
use tracing::{debug, instrument};

use crate::config::AppConfig;
use crate::error::{ImagenError, Result};
use crate::retry::with_retry;
use crate::types::{EditRequest, GenerateRequest, ImageData, ProviderResponse, UsageInfo};

use super::{api_quality, api_size, is_dall_e_3, is_gpt_image_model, ImageProvider, ModelInfo};

/// Azure OpenAI image generation provider.
pub struct AzureProvider {
    client: Client,
    endpoint: String,
    deployment: String,
    api_key: String,
    api_version: String,
    default_model: String,
}

impl AzureProvider {
    pub fn new(config: &AppConfig) -> Result<Self> {
        let endpoint = config
            .azure_endpoint
            .as_deref()
            .ok_or_else(|| ImagenError::ConfigError("Azure endpoint not configured".into()))?
            .trim_end_matches('/')
            .to_string();
        let deployment = config
            .azure_deployment_name
            .as_deref()
            .ok_or_else(|| ImagenError::ConfigError("Azure deployment name not configured".into()))?
            .to_string();
        let api_key = config
            .azure_api_key
            .as_deref()
            .ok_or_else(|| ImagenError::ConfigError("Azure API key not configured".into()))?
            .to_string();
        let api_version = config
            .azure_api_version
            .as_deref()
            .unwrap_or("2025-04-01-preview")
            .to_string();
        Ok(Self {
            client: Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .expect("failed to build reqwest client"),
            endpoint,
            deployment,
            api_key,
            api_version,
            default_model: config.default_model.clone(),
        })
    }

    fn generations_url(&self) -> String {
        format!(
            "{}/openai/deployments/{}/images/generations?api-version={}",
            self.endpoint, self.deployment, self.api_version
        )
    }

    fn edits_url(&self) -> String {
        format!(
            "{}/openai/deployments/{}/images/edits?api-version={}",
            self.endpoint, self.deployment, self.api_version
        )
    }

    fn parse_response(&self, body: serde_json::Value, model: &str) -> Result<ProviderResponse> {
        let data = body["data"]
            .as_array()
            .ok_or_else(|| ImagenError::ProviderError {
                message: "Missing 'data' in response".into(),
                status_code: None,
            })?;
        let mut images = Vec::with_capacity(data.len());
        for item in data {
            let b64_json = item["b64_json"]
                .as_str()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| ImagenError::ProviderError {
                    message: "Image response did not contain b64_json data".into(),
                    status_code: None,
                })?;
            images.push(ImageData {
                b64_json: b64_json.to_string(),
                revised_prompt: item["revised_prompt"].as_str().map(|s| s.to_string()),
            });
        }
        if images.is_empty() {
            return Err(ImagenError::ProviderError {
                message: "Image response contained no images".into(),
                status_code: None,
            });
        }
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
            .unwrap_or("Unknown Azure error");
        match status.as_u16() {
            401 | 403 => ImagenError::ProviderAuth(msg.to_string()),
            429 => ImagenError::RateLimit(msg.to_string()),
            code => ImagenError::ProviderError {
                message: format!("Azure API error ({}): {}", status, msg),
                status_code: Some(code),
            },
        }
    }
}

fn background_str(bg: &crate::types::ImageBackground) -> &'static str {
    match bg {
        crate::types::ImageBackground::Transparent => "transparent",
        crate::types::ImageBackground::Opaque => "opaque",
        crate::types::ImageBackground::Auto => "auto",
    }
}

#[async_trait]
impl ImageProvider for AzureProvider {
    #[instrument(skip(self, request), fields(provider = "azure"))]
    async fn generate(&self, request: &GenerateRequest) -> Result<ProviderResponse> {
        let model = request.model.as_deref().unwrap_or(&self.default_model);
        let size = request
            .size
            .as_ref()
            .map(|s| api_size(model, s))
            .unwrap_or_else(|| api_size(model, &crate::types::ImageSize::Auto));
        let quality = request
            .quality
            .as_ref()
            .map(|q| json!(api_quality(model, q)))
            .unwrap_or(json!("auto"));
        let n = request.n.unwrap_or(1);

        let mut body = json!({
            "prompt": request.prompt,
            "n": n,
            "size": size,
            "quality": quality
        });
        if is_dall_e_3(model) {
            if let Some(ref style) = request.style {
                body["style"] = serde_json::to_value(style).unwrap_or(json!("vivid"));
            }
        }
        if is_gpt_image_model(model) {
            body["output_format"] = json!(request
                .format
                .as_ref()
                .unwrap_or(&crate::types::OutputFormat::Png)
                .as_api_str());
        }
        if let Some(c) = request
            .output_compression
            .filter(|_| is_gpt_image_model(model))
        {
            body["output_compression"] = json!(c);
        }
        if let Some(bg) = request
            .background
            .as_ref()
            .filter(|_| is_gpt_image_model(model))
        {
            body["background"] = json!(bg.as_api_str());
        }
        if let Some(m) = request
            .moderation
            .as_ref()
            .filter(|_| is_gpt_image_model(model))
        {
            body["moderation"] = json!(m);
        }
        if let Some(ref u) = request.user {
            body["user"] = json!(u);
        }

        debug!(url = %self.generations_url(), "Sending generation request to Azure");
        let url = self.generations_url();
        let model_owned = model.to_string();
        with_retry(|| {
            let client = &self.client;
            let body = &body;
            let url = &url;
            let model_owned = &model_owned;
            async move {
                let response = client
                    .post(url.as_str())
                    .header("api-key", &self.api_key)
                    .header("Content-Type", "application/json")
                    .json(body)
                    .send()
                    .await?;
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

    #[instrument(skip(self, request), fields(provider = "azure"))]
    async fn edit(&self, request: &EditRequest) -> Result<ProviderResponse> {
        let model = request.model.as_deref().unwrap_or(&self.default_model);

        let mut all_image_bytes: Vec<(Vec<u8>, String)> = Vec::new();
        for image_path in &request.image_paths {
            let bytes = tokio::fs::read(image_path).await.map_err(|e| {
                ImagenError::FileError(format!("Failed to read image {}: {}", image_path, e))
            })?;
            let fname = std::path::Path::new(image_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("image.png")
                .to_string();
            all_image_bytes.push((bytes, fname));
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

        debug!(url = %self.edits_url(), "Sending edit request to Azure");
        let url = self.edits_url();
        let model_owned = model.to_string();
        let size = request.size.as_ref().map(|size| api_size(model, size));
        let prompt = request.prompt.clone();
        let n = request.n.unwrap_or(1);
        let output_compression = request.output_compression;
        let format = request
            .format
            .clone()
            .unwrap_or(crate::types::OutputFormat::Png);
        let quality = request
            .quality
            .as_ref()
            .map(|quality| api_quality(model, quality));
        let background = request.background.clone();
        let moderation = request.moderation.clone();
        let user = request.user.clone();

        with_retry(|| {
            let client = &self.client;
            let model_owned = &model_owned;
            let all_image_bytes = &all_image_bytes;
            let mask_bytes = &mask_bytes;
            let url = &url;
            let size = &size;
            let format = &format;
            let quality = &quality;
            let prompt = &prompt;
            let background = &background;
            let moderation = &moderation;
            let user = &user;
            async move {
                let mut form = multipart::Form::new()
                    .text("prompt", prompt.clone())
                    .text("n", n.to_string());
                if let Some(ref size) = size {
                    form = form.text("size", size.clone());
                }
                if is_gpt_image_model(model_owned) {
                    form = form.text("output_format", format.as_api_str().to_string());
                    form = form.text("quality", quality.unwrap_or("auto").to_string());
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
                if let Some(c) = output_compression.filter(|_| is_gpt_image_model(model_owned)) {
                    form = form.text("output_compression", c.to_string());
                }
                if let Some(bg) = background
                    .as_ref()
                    .filter(|_| is_gpt_image_model(model_owned))
                {
                    form = form.text("background", background_str(bg).to_string());
                }
                if let Some(m) = moderation
                    .as_ref()
                    .filter(|_| is_gpt_image_model(model_owned))
                {
                    form = form.text("moderation", m.clone());
                }
                if let Some(ref u) = *user {
                    form = form.text("user", u.clone());
                }
                let response = client
                    .post(url.as_str())
                    .header("api-key", &self.api_key)
                    .multipart(form)
                    .send()
                    .await?;
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
        vec![ModelInfo {
            id: "gpt-image-2.5-sunburst".to_string(),
            name: "GPT Image 2.5 Sunburst (Azure)".to_string(),
            supports_editing: true,
            max_images: 10,
        }]
    }

    fn provider_name(&self) -> &'static str {
        "azure"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Provider;

    fn test_config() -> AppConfig {
        AppConfig {
            provider: Provider::Azure,
            azure_endpoint: Some("https://my-resource.openai.azure.com".into()),
            azure_deployment_name: Some("gpt-image-2".into()),
            azure_api_key: Some("test-key".into()),
            azure_api_version: Some("2025-04-01-preview".into()),
            openai_api_key: None,
            openai_org_id: None,
            output_dir: "/tmp/test".into(),
            max_concurrent_jobs: 4,
            default_model: "gpt-image-2".into(),
        }
    }

    #[test]
    fn test_azure_provider_creation() {
        let provider = AzureProvider::new(&test_config()).unwrap();
        assert_eq!(
            provider.generations_url(),
            "https://my-resource.openai.azure.com/openai/deployments/gpt-image-2/images/generations?api-version=2025-04-01-preview"
        );
        assert_eq!(
            provider.edits_url(),
            "https://my-resource.openai.azure.com/openai/deployments/gpt-image-2/images/edits?api-version=2025-04-01-preview"
        );
    }

    #[test]
    fn test_azure_provider_missing_endpoint() {
        let mut config = test_config();
        config.azure_endpoint = None;
        assert!(AzureProvider::new(&config).is_err());
    }

    #[test]
    fn test_azure_get_models() {
        let provider = AzureProvider::new(&test_config()).unwrap();
        let models = provider.get_models();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "gpt-image-2.5-sunburst");
        assert!(models[0].supports_editing);
        assert_eq!(models[0].max_images, 10);
    }

    #[test]
    fn test_parse_response() {
        let provider = AzureProvider::new(&test_config()).unwrap();
        let body = json!({
            "data": [{
                "b64_json": "aW1hZ2VkYXRh",
                "revised_prompt": "A beautiful sunset over the ocean"
            }],
            "usage": { "input_tokens": 10, "output_tokens": 100 }
        });
        let response = provider.parse_response(body, "gpt-image-2").unwrap();
        assert_eq!(response.images.len(), 1);
        assert_eq!(response.images[0].b64_json, "aW1hZ2VkYXRh");
        assert_eq!(
            response.images[0].revised_prompt,
            Some("A beautiful sunset over the ocean".to_string())
        );
        assert_eq!(response.model, "gpt-image-2");
        assert!(response.usage.is_some());
    }
}
