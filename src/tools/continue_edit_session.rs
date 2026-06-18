use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::jobs::JobKind;
use crate::runtime::state::AppState;
use crate::sandbox::validate_input_path;
use crate::types::EditRequest;

/// Input parameters for the continue_edit_session tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ContinueEditSessionInput {
    /// The session ID returned from a previous edit_image call.
    pub session_id: String,
    /// Text prompt describing the next edit to apply.
    pub prompt: String,
    /// Optional path to a mask image for this edit step.
    pub mask_path: Option<String>,
}

/// Output from the continue_edit_session tool.
#[derive(Debug, Serialize)]
pub struct ContinueEditSessionOutput {
    pub job_id: String,
    pub session_id: String,
    pub step: u32,
    pub status: String,
}

/// Execute the continue_edit_session tool logic.
pub async fn run(state: &AppState, input: ContinueEditSessionInput) -> Result<String, String> {
    if input.session_id.trim().is_empty() {
        return Err("session_id cannot be empty.".to_string());
    }
    if input.prompt.trim().is_empty() {
        return Err("Prompt cannot be empty.".to_string());
    }

    // Look up session
    let session = state
        .get_edit_session(&input.session_id)
        .await
        .ok_or_else(|| {
            format!(
                "Edit session '{}' not found or expired.",
                input.session_id
            )
        })?;

    // Validate session's last image still exists and is safe
    validate_input_path(&session.last_image_path).map_err(|e| {
        format!(
            "Session image no longer valid: {}",
            e
        )
    })?;

    // Validate mask file if provided
    if let Some(ref mask) = input.mask_path {
        validate_input_path(mask).map_err(|e| e.to_string())?;
        if !tokio::fs::metadata(mask)
            .await
            .map(|m| m.is_file())
            .unwrap_or(false)
        {
            return Err(format!("Mask file not found: '{mask}'"));
        }
    }

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

    // Build edit request using the last image from the session
    let request = EditRequest {
        prompt: input.prompt,
        image_paths: vec![session.last_image_path.clone()],
        mask_path: input.mask_path,
        model: Some(model),
        size: None,
        quality: None,
        format: None,
        n: Some(1),
    };

    // Submit to provider
    let result = state.provider.edit(&request).await;
    match result {
        Ok(response) => {
            let mut results = Vec::new();
            let mut last_path = session.last_image_path.clone();
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
            state
                .upsert_edit_session(&input.session_id, &last_path)
                .await;
        }
        Err(e) => {
            let _ = state.job_registry.fail_job(&job_id, e.to_string()).await;
        }
    }

    let updated_session = state.get_edit_session(&input.session_id).await;
    let step = updated_session.map(|s| s.step_count).unwrap_or(0);

    let output = ContinueEditSessionOutput {
        job_id,
        session_id: input.session_id,
        step,
        status: "submitted".to_string(),
    };

    serde_json::to_string(&output).map_err(|e| format!("Serialization error: {e}"))
}
