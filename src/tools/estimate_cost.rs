use schemars::JsonSchema;
use serde::Deserialize;

use crate::cost;
use crate::runtime::state::AppState;
use crate::tools::parse::{parse_quality, parse_size};
use crate::types::{ImageQuality, ImageSize};

/// Input parameters for the estimate_cost tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct EstimateCostInput {
    /// Image size: "1024x1024", "1536x1024", "1024x1536", or "auto".
    pub size: Option<String>,
    /// Image quality: "standard" or "hd".
    pub quality: Option<String>,
    /// Number of images (1-4).
    pub n: Option<u8>,
    /// Operation type: "generate" or "edit".
    #[allow(dead_code)] // Part of the public API schema for future cost differentiation
    pub operation: Option<String>,
}

/// Execute the estimate_cost tool logic.
pub async fn run(state: &AppState, input: EstimateCostInput) -> Result<String, String> {
    let size = match &input.size {
        Some(s) => parse_size(s).map_err(|e| e.to_string())?,
        None => ImageSize::default(),
    };
    let quality = match &input.quality {
        Some(q) => parse_quality(q).map_err(|e| e.to_string())?,
        None => ImageQuality::default(),
    };
    let n = input.n.unwrap_or(1);
    if n == 0 || n > 4 {
        return Err("n must be between 1 and 4.".to_string());
    }

    let provider_name = state.provider.provider_name();
    let model = state.config.default_model.clone();

    let estimate = cost::estimate_cost(provider_name, &model, &size, &quality, n);

    serde_json::to_string(&estimate).map_err(|e| format!("Serialization error: {e}"))
}
