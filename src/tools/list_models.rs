use serde::Serialize;

use crate::providers::ModelInfo;
use crate::runtime::state::AppState;

/// Output from the list_models tool.
#[derive(Debug, Serialize)]
pub struct ListModelsOutput {
    pub provider: String,
    pub models: Vec<ModelInfo>,
}

/// Execute the list_models tool logic.
pub async fn run(state: &AppState) -> Result<String, String> {
    let models = state.provider.get_models();
    let output = ListModelsOutput {
        provider: state.provider.provider_name().to_string(),
        models,
    };

    serde_json::to_string(&output).map_err(|e| format!("Serialization error: {e}"))
}
