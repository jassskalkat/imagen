use serde::Serialize;

use crate::runtime::state::AppState;

/// Output from the get_config tool (never exposes API keys).
#[derive(Debug, Serialize)]
pub struct GetConfigOutput {
    pub provider: String,
    pub output_dir: String,
    pub max_concurrent_jobs: usize,
    pub default_model: String,
    pub available_models: Vec<ModelSummary>,
}

/// Summary of an available model.
#[derive(Debug, Serialize)]
pub struct ModelSummary {
    pub id: String,
    pub name: String,
    pub supports_editing: bool,
}

/// Execute the get_config tool logic.
pub async fn run(state: &AppState) -> Result<String, String> {
    let provider_name = state.provider.provider_name().to_string();
    let models = state.provider.get_models();

    let output = GetConfigOutput {
        provider: provider_name,
        output_dir: state.config.output_dir.clone(),
        max_concurrent_jobs: state.config.max_concurrent_jobs,
        default_model: state.config.default_model.clone(),
        available_models: models
            .into_iter()
            .map(|m| ModelSummary {
                id: m.id,
                name: m.name,
                supports_editing: m.supports_editing,
            })
            .collect(),
    };

    serde_json::to_string(&output).map_err(|e| format!("Serialization error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, Provider};
    use crate::error::Result;
    use crate::providers::{ImageProvider, ModelInfo};
    use crate::types::{EditRequest, GenerateRequest, ProviderResponse};
    use async_trait::async_trait;
    use std::sync::Arc;

    struct MockProvider;

    #[async_trait]
    impl ImageProvider for MockProvider {
        async fn generate(&self, _request: &GenerateRequest) -> Result<ProviderResponse> {
            Ok(ProviderResponse {
                images: vec![],
                model: "mock".to_string(),
                usage: None,
            })
        }

        async fn edit(&self, _request: &EditRequest) -> Result<ProviderResponse> {
            Ok(ProviderResponse {
                images: vec![],
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

    fn test_state_with_keys() -> AppState {
        let config = AppConfig {
            provider: Provider::OpenAI,
            azure_endpoint: None,
            azure_deployment_name: None,
            azure_api_key: Some("azure-secret-key-12345".into()),
            azure_api_version: None,
            openai_api_key: Some("sk-openai-secret-key-67890".into()),
            openai_org_id: Some("org-secret-id".into()),
            output_dir: "/tmp/test-output".into(),
            max_concurrent_jobs: 4,
            default_model: "gpt-image-2".into(),
        };
        let provider = Arc::new(MockProvider);
        AppState::new(config, provider)
    }

    #[tokio::test]
    async fn test_get_config_does_not_leak_api_keys() {
        let state = test_state_with_keys();
        let output = run(&state).await.unwrap();

        // The output must never contain any API key values
        assert!(
            !output.contains("azure-secret-key-12345"),
            "Output leaked azure_api_key"
        );
        assert!(
            !output.contains("sk-openai-secret-key-67890"),
            "Output leaked openai_api_key"
        );
        assert!(
            !output.contains("org-secret-id"),
            "Output leaked openai_org_id"
        );

        // Verify it does contain expected safe fields
        assert!(output.contains("gpt-image-2"));
        assert!(output.contains("/tmp/test-output"));
        assert!(output.contains("mock"));
    }
}
