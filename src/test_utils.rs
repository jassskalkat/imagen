//! Shared test utilities — only compiled in `#[cfg(test)]` contexts.

use std::sync::Arc;

use async_trait::async_trait;

use crate::config::{AppConfig, Provider};
use crate::error::Result;
use crate::providers::{ImageProvider, ModelInfo};
use crate::runtime::state::AppState;
use crate::types::{EditRequest, GenerateRequest, ImageData, ProviderResponse};

/// Minimal mock provider that returns one transparent 1×1 image per request.
pub struct MockProvider;

#[async_trait]
impl ImageProvider for MockProvider {
    async fn generate(&self, _request: &GenerateRequest) -> Result<ProviderResponse> {
        Ok(ProviderResponse {
            // "dGVzdA==" decodes to b"test" — a valid non-empty payload.
            images: vec![ImageData {
                b64_json: "dGVzdA==".to_string(),
                revised_prompt: Some("A test image".to_string()),
            }],
            model: "mock".to_string(),
            usage: None,
        })
    }

    async fn edit(&self, _request: &EditRequest) -> Result<ProviderResponse> {
        Ok(ProviderResponse {
            images: vec![ImageData {
                b64_json: "dGVzdA==".to_string(),
                revised_prompt: None,
            }],
            model: "mock".to_string(),
            usage: None,
        })
    }

    fn get_models(&self) -> Vec<ModelInfo> {
        vec![ModelInfo {
            id: "mock-model".to_string(),
            name: "Mock Model".to_string(),
            supports_editing: true,
            max_images: 4,
        }]
    }

    fn provider_name(&self) -> &'static str {
        "mock"
    }
}

/// A mock provider that always fails every call.
pub struct FailingMockProvider;

#[async_trait]
impl ImageProvider for FailingMockProvider {
    async fn generate(&self, _request: &GenerateRequest) -> Result<ProviderResponse> {
        Err(crate::error::ImagenError::ProviderError {
            message: "service unavailable".into(),
            status_code: Some(503),
        })
    }
    async fn edit(&self, _request: &EditRequest) -> Result<ProviderResponse> {
        Err(crate::error::ImagenError::ProviderError {
            message: "service unavailable".into(),
            status_code: Some(503),
        })
    }
    fn get_models(&self) -> Vec<ModelInfo> {
        vec![]
    }
    fn provider_name(&self) -> &'static str {
        "mock"
    }
}

/// Build a minimal `AppConfig` backed by `MockProvider`.
pub fn mock_config(output_dir: &str) -> AppConfig {
    AppConfig {
        provider: Provider::OpenAI,
        azure_endpoint: None,
        azure_deployment_name: None,
        azure_api_key: None,
        azure_api_version: None,
        openai_api_key: Some("sk-test".into()),
        openai_org_id: None,
        output_dir: output_dir.to_string(),
        max_concurrent_jobs: 2,
        default_model: "gpt-image-2".into(),
    }
}

/// Build an `AppState` with `MockProvider` and a temp output directory.
pub fn mock_state(subdir: &str) -> AppState {
    let output_dir = std::env::temp_dir()
        .join(subdir)
        .to_string_lossy()
        .to_string();
    AppState::new(mock_config(&output_dir), Arc::new(MockProvider))
}
