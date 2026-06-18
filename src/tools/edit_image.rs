use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::jobs::JobKind;
use crate::runtime::state::AppState;
use crate::sandbox::validate_input_path;
use crate::tools::generate_image::{parse_quality, parse_size};
use crate::types::EditRequest;

/// Input parameters for the edit_image tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct EditImageInput {
    /// Path to the source image file to edit.
    pub image_path: String,
    /// Text prompt describing the desired edits.
    pub prompt: String,
    /// Optional path to a mask image (white areas will be edited).
    pub mask_path: Option<String>,
    /// Image size: "1024x1024", "1536x1024", "1024x1536", or "auto".
    pub size: Option<String>,
    /// Image quality: "standard" or "hd".
    pub quality: Option<String>,
}

/// Output from the edit_image tool.
#[derive(Debug, Serialize)]
pub struct EditImageOutput {
    pub job_id: String,
    pub session_id: String,
    pub status: String,
}

/// Execute the edit_image tool logic.
pub async fn run(state: &AppState, input: EditImageInput) -> Result<String, String> {
    if input.prompt.trim().is_empty() {
        return Err("Prompt cannot be empty.".to_string());
    }
    if input.image_path.trim().is_empty() {
        return Err("image_path cannot be empty.".to_string());
    }

    // Validate input paths for path traversal and null bytes
    validate_input_path(&input.image_path).map_err(|e| e.to_string())?;
    if let Some(ref mask) = input.mask_path {
        validate_input_path(mask).map_err(|e| e.to_string())?;
    }

    // Validate image file exists
    if !tokio::fs::metadata(&input.image_path)
        .await
        .map(|m| m.is_file())
        .unwrap_or(false)
    {
        return Err(format!(
            "Image file not found: '{}'",
            input.image_path
        ));
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

    let provider_name = state.provider.provider_name();
    let model = state.config.default_model.clone();

    // Create job and immediately mark as Running to prevent the background
    // worker from picking it up (avoids double-execution race).
    let job_id = state
        .job_registry
        .create_job(JobKind::Edit, provider_name, &model, &input.prompt)
        .await;
    let _ = state
        .job_registry
        .update_status(&job_id, crate::jobs::JobStatus::Running)
        .await;

    // Create a new edit session
    let session_id = Uuid::new_v4().to_string();

    // Build edit request
    let request = EditRequest {
        prompt: input.prompt,
        image_paths: vec![input.image_path.clone()],
        mask_path: input.mask_path,
        model: Some(model),
        size,
        quality,
        format: None,
        n: Some(1),
    };

    // Submit to provider
    let result = state.provider.edit(&request).await;
    match result {
        Ok(response) => {
            let mut results = Vec::new();
            let mut last_path = input.image_path.clone();
            for (i, img) in response.images.iter().enumerate() {
                let bytes = base64::Engine::decode(
                    &base64::engine::general_purpose::STANDARD,
                    &img.b64_json,
                )
                .map_err(|e| format!("Failed to decode image data: {e}"))?;

                let fmt = crate::types::OutputFormat::default();
                let path = crate::artifacts::artifact_path(
                    &state.config.output_dir,
                    &job_id,
                    i as u32,
                    &fmt,
                );
                crate::artifacts::save_artifact(&path, &bytes, &state.config.output_dir)
                    .await
                    .map_err(|e| format!("Failed to save artifact: {e}"))?;

                last_path = path.to_string_lossy().to_string();
                results.push(crate::types::ImageResult {
                    file_path: last_path.clone(),
                    format: fmt,
                    size_bytes: bytes.len() as u64,
                    revised_prompt: img.revised_prompt.clone(),
                });
            }
            let _ = state.job_registry.complete_job(&job_id, results).await;
            // Record session with the last produced image
            state.upsert_edit_session(&session_id, &last_path).await;
        }
        Err(e) => {
            let _ = state.job_registry.fail_job(&job_id, e.to_string()).await;
            // Still record session with original image
            state
                .upsert_edit_session(&session_id, &input.image_path)
                .await;
        }
    }

    let output = EditImageOutput {
        job_id,
        session_id,
        status: "submitted".to_string(),
    };

    serde_json::to_string(&output).map_err(|e| format!("Serialization error: {e}"))
}
