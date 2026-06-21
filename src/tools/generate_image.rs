use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::{info, instrument, warn};

use crate::cost;
use crate::jobs::JobKind;
use crate::runtime::state::AppState;
use crate::tools::parse::{
    parse_background, parse_compression, parse_format, parse_moderation, parse_quality, parse_size,
    parse_style,
};
use crate::types::{GenerateRequest, ImageQuality, ImageSize, ImageStyle, OutputFormat};

/// Input parameters for the generate_image tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GenerateImageInput {
    /// The text prompt describing the image to generate.
    pub prompt: String,
    /// Image size: "1024x1024", "1536x1024", "1024x1536", "auto", or arbitrary "WxH".
    pub size: Option<String>,
    /// Image quality: "low", "medium", "high", "auto", "standard", or "hd".
    pub quality: Option<String>,
    /// Image style: "vivid" or "natural". Ignored for gpt-image-2.
    pub style: Option<String>,
    /// Output format: "png", "webp", or "jpeg".
    pub output_format: Option<String>,
    /// Number of images to generate (1-10).
    pub n: Option<u8>,
    /// Output compression percentage (0-100).
    pub output_compression: Option<u8>,
    /// Image background: "opaque" or "auto".
    pub background: Option<String>,
    /// Content moderation level: "low" or "auto".
    pub moderation: Option<String>,
    /// User tracking identifier passed to the API.
    pub user: Option<String>,
}

/// Output from the generate_image tool.
#[derive(Debug, Serialize)]
pub struct GenerateImageOutput {
    pub job_id: String,
    pub status: String,
    pub cost_estimate: cost::CostEstimate,
}

/// Maximum prompt length in bytes accepted by the server.
const MAX_PROMPT_LEN: usize = 4000;

/// Execute the generate_image tool logic.
#[instrument(skip(state), fields(prompt_len = input.prompt.len(), n = input.n.unwrap_or(1)))]
pub async fn run(state: &AppState, input: GenerateImageInput) -> Result<String, String> {
    if input.prompt.trim().is_empty() {
        return Err("Prompt cannot be empty.".to_string());
    }
    if input.prompt.len() > MAX_PROMPT_LEN {
        return Err(format!(
            "Prompt exceeds maximum length of {MAX_PROMPT_LEN} characters."
        ));
    }

    let size = match &input.size {
        Some(s) => parse_size(s).map_err(|e| e.to_string())?,
        None => ImageSize::default(),
    };
    let quality = match &input.quality {
        Some(q) => parse_quality(q).map_err(|e| e.to_string())?,
        None => ImageQuality::default(),
    };
    let style = match &input.style {
        Some(s) => parse_style(s).map_err(|e| e.to_string())?,
        None => ImageStyle::default(),
    };
    let format = match &input.output_format {
        Some(f) => parse_format(f).map_err(|e| e.to_string())?,
        None => OutputFormat::default(),
    };
    let background = match &input.background {
        Some(b) => Some(parse_background(b).map_err(|e| e.to_string())?),
        None => None,
    };
    let output_compression = match input.output_compression {
        Some(c) => Some(parse_compression(c).map_err(|e| e.to_string())?),
        None => None,
    };
    let moderation = match &input.moderation {
        Some(m) => Some(parse_moderation(m).map_err(|e| e.to_string())?),
        None => None,
    };
    let n = input.n.unwrap_or(1);
    if n == 0 || n > 10 {
        return Err("n must be between 1 and 10.".to_string());
    }

    let provider_name = state.provider.provider_name();
    let model = state.config.default_model.clone();

    // Create job directly in Running state so the background worker never sees
    // a Queued entry for this inline-processed job (eliminates TOCTOU race).
    let job_id = state
        .job_registry
        .create_job_running(JobKind::Generate, provider_name, &model, &input.prompt)
        .await;

    info!(job_id = %job_id, model = %model, "generate_image job started");

    // For gpt-image-2, style is not supported - suppress it
    let effective_style = if model.contains("gpt-image") {
        None
    } else {
        Some(style)
    };

    // Build the provider request
    let request = GenerateRequest {
        prompt: input.prompt,
        model: Some(model.clone()),
        size: Some(size.clone()),
        quality: Some(quality.clone()),
        format: Some(format),
        style: effective_style,
        n: Some(n),
        output_compression,
        background,
        moderation,
        user: input.user,
    };

    // Submit to provider (synchronous for now, worker can pick up later)
    let result = state.provider.generate(&request).await;
    let estimate = cost::estimate_cost(provider_name, &model, &size, &quality, n);

    let fmt = request.format.clone().unwrap_or_default();
    let status = match result {
        Ok(response) => {
            match crate::artifacts::save_provider_response(
                &state.config.output_dir,
                &job_id,
                &response,
                &fmt,
            )
            .await
            {
                Ok(results) => {
                    info!(job_id = %job_id, count = results.len(), "generate_image job completed");
                    let _ = state.job_registry.complete_job(&job_id, results).await;
                    "submitted".to_string()
                }
                Err(e) => {
                    let msg = e.to_string();
                    warn!(job_id = %job_id, error = %msg, "generate_image artifact save failed");
                    let _ = state.job_registry.fail_job(&job_id, msg.clone()).await;
                    format!("failed: {msg}")
                }
            }
        }
        Err(e) => {
            let error_msg = e.to_string();
            warn!(job_id = %job_id, error = %error_msg, "generate_image provider call failed");
            let _ = state
                .job_registry
                .fail_job(&job_id, error_msg.clone())
                .await;
            format!("failed: {error_msg}")
        }
    };

    let output = GenerateImageOutput {
        job_id,
        status,
        cost_estimate: estimate,
    };

    serde_json::to_string(&output).map_err(|e| format!("Serialization error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::config::Provider;
    use crate::runtime::state::AppState;
    use crate::test_utils::{mock_state, FailingMockProvider};
    use std::sync::Arc;

    fn test_state() -> AppState {
        mock_state("imagen-gen-test")
    }

    fn make_input(prompt: &str) -> GenerateImageInput {
        GenerateImageInput {
            prompt: prompt.to_string(),
            size: None,
            quality: None,
            style: None,
            output_format: None,
            n: None,
            output_compression: None,
            background: None,
            moderation: None,
            user: None,
        }
    }

    #[tokio::test]
    async fn test_empty_prompt_returns_error() {
        let result = run(&test_state(), make_input("   ")).await;
        assert!(result.unwrap_err().contains("Prompt cannot be empty"));
    }

    #[tokio::test]
    async fn test_invalid_size_returns_error() {
        let mut input = make_input("A cat");
        input.size = Some("999x999".to_string());
        let result = run(&test_state(), input).await;
        assert!(result.unwrap_err().contains("Invalid size"));
    }

    #[tokio::test]
    async fn test_invalid_quality_returns_error() {
        let mut input = make_input("A cat");
        input.quality = Some("ultra".to_string());
        let result = run(&test_state(), input).await;
        assert!(result.unwrap_err().contains("Invalid quality"));
    }

    #[tokio::test]
    async fn test_n_zero_returns_error() {
        let mut input = make_input("A cat");
        input.n = Some(0);
        let result = run(&test_state(), input).await;
        assert!(result.unwrap_err().contains("n must be between 1 and 10"));
    }

    #[tokio::test]
    async fn test_n_ten_is_valid() {
        let mut input = make_input("A cat");
        input.n = Some(10);
        let result = run(&test_state(), input).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_n_eleven_returns_error() {
        let mut input = make_input("A cat");
        input.n = Some(11);
        let result = run(&test_state(), input).await;
        assert!(result.unwrap_err().contains("n must be between 1 and 10"));
    }

    #[tokio::test]
    async fn test_compression_invalid() {
        let mut input = make_input("A cat");
        input.output_compression = Some(101);
        let result = run(&test_state(), input).await;
        assert!(result.unwrap_err().contains("output_compression"));
    }

    #[tokio::test]
    async fn test_provider_failure_returns_failed_status() {
        let config = AppConfig {
            provider: Provider::OpenAI,
            azure_endpoint: None,
            azure_deployment_name: None,
            azure_api_key: None,
            azure_api_version: None,
            openai_api_key: Some("sk-test".into()),
            openai_org_id: None,
            output_dir: std::env::temp_dir()
                .join("imagen-gen-fail-test")
                .to_string_lossy()
                .to_string(),
            max_concurrent_jobs: 2,
            default_model: "gpt-image-2".into(),
        };
        let state = AppState::new(config, Arc::new(FailingMockProvider));
        let result = run(&state, make_input("A cat")).await;
        assert!(result.is_ok(), "Should return Ok with failed status");
        let output = result.unwrap();
        assert!(
            output.contains("\"status\":\"failed:"),
            "Status should indicate failure, got: {output}"
        );
    }
}
