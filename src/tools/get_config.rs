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
    use crate::test_utils::{mock_config, MockProvider};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_get_config_does_not_leak_api_keys() {
        let mut config = mock_config("/tmp/test-output");
        config.azure_api_key = Some("azure-secret-key-12345".into());
        config.openai_api_key = Some("sk-openai-secret-key-67890".into());
        config.openai_org_id = Some("org-secret-id".into());
        let state = AppState::new(config, Arc::new(MockProvider));
        let output = run(&state).await.unwrap();

        assert!(!output.contains("azure-secret-key-12345"), "Output leaked azure_api_key");
        assert!(!output.contains("sk-openai-secret-key-67890"), "Output leaked openai_api_key");
        assert!(!output.contains("org-secret-id"), "Output leaked openai_org_id");
        assert!(output.contains("gpt-image-2"));
        assert!(output.contains("/tmp/test-output"));
        assert!(output.contains("mock"));
    }
}
