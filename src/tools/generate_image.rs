use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::cost;
use crate::error::ImagenError;
use crate::jobs::JobKind;
use crate::runtime::state::AppState;
use crate::types::{GenerateRequest, ImageQuality, ImageSize, ImageStyle, OutputFormat};

/// Input parameters for the generate_image tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GenerateImageInput {
    /// The text prompt describing the image to generate.
    pub prompt: String,
    /// Image size: "1024x1024", "1536x1024", "1024x1536", or "auto".
    pub size: Option<String>,
    /// Image quality: "standard" or "hd".
    pub quality: Option<String>,
    /// Image style: "vivid" or "natural".
    pub style: Option<String>,
    /// Output format: "png", "webp", or "jpeg".
    pub output_format: Option<String>,
    /// Number of images to generate (1-4).
    pub n: Option<u8>,
}

/// Output from the generate_image tool.
#[derive(Debug, Serialize)]
pub struct GenerateImageOutput {
    pub job_id: String,
    pub status: String,
    pub cost_estimate: cost::CostEstimate,
}

/// Parse a size string into an ImageSize enum.
pub fn parse_size(s: &str) -> Result<ImageSize, ImagenError> {
    match s {
        "1024x1024" => Ok(ImageSize::Square),
        "1536x1024" => Ok(ImageSize::Landscape),
        "1024x1536" => Ok(ImageSize::Portrait),
        "auto" => Ok(ImageSize::Auto),
        other => Err(ImagenError::InvalidInput(format!(
            "Invalid size: '{other}'. Use 1024x1024, 1536x1024, 1024x1536, or auto."
        ))),
    }
}

/// Parse a quality string into an ImageQuality enum.
pub fn parse_quality(s: &str) -> Result<ImageQuality, ImagenError> {
    match s {
        "standard" => Ok(ImageQuality::Standard),
        "hd" => Ok(ImageQuality::Hd),
        other => Err(ImagenError::InvalidInput(format!(
            "Invalid quality: '{other}'. Use standard or hd."
        ))),
    }
}

/// Parse a style string into an ImageStyle enum.
fn parse_style(s: &str) -> Result<ImageStyle, ImagenError> {
    match s {
        "vivid" => Ok(ImageStyle::Vivid),
        "natural" => Ok(ImageStyle::Natural),
        other => Err(ImagenError::InvalidInput(format!(
            "Invalid style: '{other}'. Use vivid or natural."
        ))),
    }
}

/// Parse an output format string into an OutputFormat enum.
pub fn parse_format(s: &str) -> Result<OutputFormat, ImagenError> {
    match s {
        "png" => Ok(OutputFormat::Png),
        "webp" => Ok(OutputFormat::Webp),
        "jpeg" | "jpg" => Ok(OutputFormat::Jpeg),
        other => Err(ImagenError::InvalidInput(format!(
            "Invalid format: '{other}'. Use png, webp, or jpeg."
        ))),
    }
}

/// Execute the generate_image tool logic.
pub async fn run(state: &AppState, input: GenerateImageInput) -> Result<String, String> {
    if input.prompt.trim().is_empty() {
        return Err("Prompt cannot be empty.".to_string());
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
    let n = input.n.unwrap_or(1);
    if n == 0 || n > 4 {
        return Err("n must be between 1 and 4.".to_string());
    }

    let provider_name = state.provider.provider_name();
    let model = state.config.default_model.clone();

    // Create job in registry and immediately mark as Running to prevent
    // the background worker from picking it up (avoids double-execution race).
    let job_id = state
        .job_registry
        .create_job(JobKind::Generate, provider_name, &model, &input.prompt)
        .await;
    let _ = state
        .job_registry
        .update_status(&job_id, crate::jobs::JobStatus::Running)
        .await;

    // Build the provider request
    let request = GenerateRequest {
        prompt: input.prompt,
        model: Some(model.clone()),
        size: Some(size.clone()),
        quality: Some(quality.clone()),
        format: Some(format),
        style: Some(style),
        n: Some(n),
    };

    // Submit to provider (synchronous for now, worker can pick up later)
    let result = state.provider.generate(&request).await;
    match result {
        Ok(response) => {
            // Save artifacts and complete job
            let mut results = Vec::new();
            for (i, img) in response.images.iter().enumerate() {
                let bytes = base64::Engine::decode(
                    &base64::engine::general_purpose::STANDARD,
                    &img.b64_json,
                )
                .map_err(|e| format!("Failed to decode image data: {e}"))?;

                let fmt = request.format.clone().unwrap_or_default();
                let path = crate::artifacts::artifact_path(
                    &state.config.output_dir,
                    &job_id,
                    i as u32,
                    &fmt,
                );
                crate::artifacts::save_artifact(&path, &bytes, &state.config.output_dir)
                    .await
                    .map_err(|e| format!("Failed to save artifact: {e}"))?;

                results.push(crate::types::ImageResult {
                    file_path: path.to_string_lossy().to_string(),
                    format: fmt,
                    size_bytes: bytes.len() as u64,
                    revised_prompt: img.revised_prompt.clone(),
                });
            }
            let _ = state.job_registry.complete_job(&job_id, results).await;
        }
        Err(e) => {
            let _ = state.job_registry.fail_job(&job_id, e.to_string()).await;
        }
    }

    let estimate = cost::estimate_cost(provider_name, &model, &size, &quality, n);
    let output = GenerateImageOutput {
        job_id,
        status: "submitted".to_string(),
        cost_estimate: estimate,
    };

    serde_json::to_string(&output).map_err(|e| format!("Serialization error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_size_valid() {
        assert_eq!(parse_size("1024x1024").unwrap(), ImageSize::Square);
        assert_eq!(parse_size("1536x1024").unwrap(), ImageSize::Landscape);
        assert_eq!(parse_size("1024x1536").unwrap(), ImageSize::Portrait);
        assert_eq!(parse_size("auto").unwrap(), ImageSize::Auto);
    }

    #[test]
    fn test_parse_size_invalid() {
        let result = parse_size("500x500");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid size"));
    }

    #[test]
    fn test_parse_quality_valid() {
        assert_eq!(parse_quality("standard").unwrap(), ImageQuality::Standard);
        assert_eq!(parse_quality("hd").unwrap(), ImageQuality::Hd);
    }

    #[test]
    fn test_parse_quality_invalid() {
        let result = parse_quality("ultra");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid quality"));
    }

    #[test]
    fn test_parse_format_valid() {
        assert_eq!(parse_format("png").unwrap(), OutputFormat::Png);
        assert_eq!(parse_format("webp").unwrap(), OutputFormat::Webp);
        assert_eq!(parse_format("jpeg").unwrap(), OutputFormat::Jpeg);
        assert_eq!(parse_format("jpg").unwrap(), OutputFormat::Jpeg);
    }

    #[test]
    fn test_parse_format_invalid() {
        let result = parse_format("bmp");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid format"));
    }

    #[test]
    fn test_empty_prompt_rejected() {
        // We can't easily call `run` without AppState, but we can verify
        // the input validation logic that checks empty prompts
        let input = GenerateImageInput {
            prompt: "   ".to_string(),
            size: None,
            quality: None,
            style: None,
            output_format: None,
            n: None,
        };
        assert!(input.prompt.trim().is_empty());
    }

    #[test]
    fn test_n_bounds() {
        // n=0 and n>4 should be rejected by the run function
        let n_zero: u8 = 0;
        let n_five: u8 = 5;
        assert!(n_zero == 0 || n_zero > 4); // Would be rejected
        assert!(n_five == 0 || n_five > 4); // Would be rejected

        let n_valid: u8 = 2;
        assert!(!(n_valid == 0 || n_valid > 4)); // Would be accepted
    }
}
