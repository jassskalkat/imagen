use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::{info, instrument, warn};
use uuid::Uuid;

use crate::jobs::JobKind;
use crate::runtime::state::AppState;
use crate::sandbox::validate_input_path;
use crate::tools::parse::{
    parse_background, parse_compression, parse_moderation, parse_quality, parse_size,
};
use crate::types::EditRequest;

/// Input parameters for the edit_image tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct EditImageInput {
    /// Path to the source image file to edit.
    pub image_path: String,
    /// Additional image paths for multi-image edits (up to 16 total).
    pub additional_image_paths: Option<Vec<String>>,
    /// Text prompt describing the desired edits.
    pub prompt: String,
    /// Optional path to a mask image (white areas will be edited).
    pub mask_path: Option<String>,
    /// Image size: "1024x1024", "1536x1024", "1024x1536", "auto", or arbitrary "WxH".
    pub size: Option<String>,
    /// Image quality: "low", "medium", "high", "auto", "standard", or "hd".
    pub quality: Option<String>,
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

/// Output from the edit_image tool.
#[derive(Debug, Serialize)]
pub struct EditImageOutput {
    pub job_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub status: String,
}

/// Maximum prompt length in bytes accepted by the server.
const MAX_PROMPT_LEN: usize = 4000;

/// Execute the edit_image tool logic.
#[instrument(skip(state), fields(prompt_len = input.prompt.len()))]
pub async fn run(state: &AppState, input: EditImageInput) -> Result<String, String> {
    if input.prompt.trim().is_empty() {
        return Err("Prompt cannot be empty.".to_string());
    }
    if input.prompt.len() > MAX_PROMPT_LEN {
        return Err(format!("Prompt exceeds maximum length of {MAX_PROMPT_LEN} characters."));
    }
    if input.image_path.trim().is_empty() {
        return Err("image_path cannot be empty.".to_string());
    }

    // Build combined image paths
    let mut image_paths = vec![input.image_path.clone()];
    if let Some(ref additional) = input.additional_image_paths {
        image_paths.extend(additional.iter().cloned());
    }
    if image_paths.len() > 16 {
        return Err("Total number of images cannot exceed 16.".to_string());
    }

    // Validate n
    let n = input.n.unwrap_or(1);
    if n == 0 || n > 10 {
        return Err("n must be between 1 and 10.".to_string());
    }

    // Validate compression
    let output_compression = match input.output_compression {
        Some(c) => Some(parse_compression(c).map_err(|e| e.to_string())?),
        None => None,
    };

    // Validate all input paths for path traversal and null bytes
    for path in &image_paths {
        validate_input_path(path).await.map_err(|e| e.to_string())?;
    }
    if let Some(ref mask) = input.mask_path {
        validate_input_path(mask).await.map_err(|e| e.to_string())?;
    }

    // Validate all image files exist
    for path in &image_paths {
        if !tokio::fs::metadata(path)
            .await
            .map(|m| m.is_file())
            .unwrap_or(false)
        {
            return Err(format!("Image file not found: '{path}'"));
        }
    }

    // Validate mask file if provided
    if let Some(ref mask) = input.mask_path {
        if !tokio::fs::metadata(mask)
            .await
            .map(|m| m.is_file())
            .unwrap_or(false)
        {
            return Err(format!("Mask file not found: '{mask}'"));
        }
    }

    let size = match &input.size {
        Some(s) => Some(parse_size(s).map_err(|e| e.to_string())?),
        None => None,
    };
    let quality = match &input.quality {
        Some(q) => Some(parse_quality(q).map_err(|e| e.to_string())?),
        None => None,
    };
    let background = match &input.background {
        Some(b) => Some(parse_background(b).map_err(|e| e.to_string())?),
        None => None,
    };
    let moderation = match &input.moderation {
        Some(m) => Some(parse_moderation(m).map_err(|e| e.to_string())?),
        None => None,
    };

    let provider_name = state.provider.provider_name();
    let model = state.config.default_model.clone();

    // Create job directly in Running state to eliminate the TOCTOU race.
    let job_id = state
        .job_registry
        .create_job_running(JobKind::Edit, provider_name, &model, &input.prompt)
        .await;

    info!(job_id = %job_id, model = %model, "edit_image job started");

    // Build edit request
    let request = EditRequest {
        prompt: input.prompt,
        image_paths: image_paths.clone(),
        mask_path: input.mask_path,
        model: Some(model),
        size,
        quality,
        format: None,
        n: Some(n),
        output_compression,
        background,
        moderation,
        user: input.user,
    };

    // Submit to provider
    let result = state.provider.edit(&request).await;
    let fmt = crate::types::OutputFormat::default();
    let session_id = match result {
        Ok(response) => {
            let sid = Uuid::new_v4().to_string();
            let last_path = match crate::artifacts::save_provider_response(
                &state.config.output_dir,
                &job_id,
                &response,
                &fmt,
            )
            .await
            {
                Ok(results) => {
                    let last = results.last().map(|r| r.file_path.clone()).unwrap_or_else(|| input.image_path.clone());
                    info!(job_id = %job_id, count = results.len(), "edit_image job completed");
                    let _ = state.job_registry.complete_job(&job_id, results).await;
                    last
                }
                Err(e) => {
                    warn!(job_id = %job_id, error = %e, "edit_image artifact save failed");
                    let _ = state.job_registry.fail_job(&job_id, e.to_string()).await;
                    input.image_path.clone()
                }
            };
            // Only create/update the session when the edit actually produced output.
            state.upsert_edit_session(&sid, &last_path).await;
            Some(sid)
        }
        Err(e) => {
            warn!(job_id = %job_id, error = %e, "edit_image provider call failed");
            let _ = state.job_registry.fail_job(&job_id, e.to_string()).await;
            // Do NOT create a session — the edit failed and there is no new image.
            None
        }
    };

    let output = EditImageOutput {
        job_id,
        session_id,
        status: "submitted".to_string(),
    };

    serde_json::to_string(&output).map_err(|e| format!("Serialization error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::mock_state;

    fn test_state() -> AppState {
        mock_state("imagen-edit-test")
    }

    fn make_input(image_path: &str, prompt: &str) -> EditImageInput {
        EditImageInput {
            image_path: image_path.to_string(),
            additional_image_paths: None,
            prompt: prompt.to_string(),
            mask_path: None,
            size: None,
            quality: None,
            n: None,
            output_compression: None,
            background: None,
            moderation: None,
            user: None,
        }
    }

    #[tokio::test]
    async fn test_empty_prompt_returns_error() {
        let result = run(&test_state(), make_input("/tmp/image.png", "   ")).await;
        assert!(result.unwrap_err().contains("Prompt cannot be empty"));
    }

    #[tokio::test]
    async fn test_empty_image_path_returns_error() {
        let result = run(&test_state(), make_input("", "Edit this")).await;
        assert!(result.unwrap_err().contains("image_path cannot be empty"));
    }

    #[tokio::test]
    async fn test_nonexistent_image_path_returns_error() {
        let input = make_input("/tmp/nonexistent-imagen-test-image-xyz.png", "Edit this");
        let err = run(&test_state(), input).await.unwrap_err();
        assert!(
            err.contains("not found") || err.contains("does not exist"),
            "Expected 'not found' error, got: {err}"
        );
    }

    #[tokio::test]
    async fn test_nonexistent_mask_path_returns_error() {
        let tmp_image = std::env::temp_dir().join("imagen-edit-test-img.png");
        tokio::fs::write(&tmp_image, b"fake image data")
            .await
            .unwrap();

        let mut input = make_input(&tmp_image.to_string_lossy(), "Edit this");
        input.mask_path = Some("/tmp/nonexistent-imagen-test-mask-xyz.png".to_string());
        let err = run(&test_state(), input).await.unwrap_err();
        assert!(
            err.contains("not found") || err.contains("does not exist"),
            "Expected 'not found' error, got: {err}"
        );
        let _ = tokio::fs::remove_file(&tmp_image).await;
    }

    #[tokio::test]
    async fn test_too_many_images_returns_error() {
        let mut input = make_input("/tmp/img.png", "Edit this");
        input.additional_image_paths = Some(vec!["/tmp/img.png".to_string(); 16]);
        let result = run(&test_state(), input).await;
        assert!(result.unwrap_err().contains("cannot exceed 16"));
    }

    #[tokio::test]
    async fn test_n_eleven_returns_error() {
        let mut input = make_input("/tmp/img.png", "Edit this");
        input.n = Some(11);
        let result = run(&test_state(), input).await;
        assert!(result.unwrap_err().contains("n must be between 1 and 10"));
    }
}
