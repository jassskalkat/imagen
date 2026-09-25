use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::{info, instrument, warn};
use uuid::Uuid;

use crate::jobs::JobKind;
use crate::providers::{validate_request_options, RequestOptions};
use crate::runtime::state::AppState;
use crate::sandbox::validate_input_path;
use crate::tools::parse::{
    parse_background, parse_compression, parse_format, parse_moderation, parse_quality, parse_size,
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
    /// Image quality: "low", "medium", "high", "xhigh", "max", "auto", "standard", or "hd".
    pub quality: Option<String>,
    /// Output format: "png", "webp", or "jpeg".
    pub output_format: Option<String>,
    /// Number of images to generate (1-10).
    pub n: Option<u8>,
    /// Output compression percentage (0-100).
    pub output_compression: Option<u8>,
    /// Image background: "transparent", "opaque", or "auto".
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
const MAX_PROMPT_LEN: usize = 32_000;

/// Execute the edit_image tool logic.
#[instrument(skip(state), fields(prompt_len = input.prompt.len()))]
pub async fn run(state: &AppState, input: EditImageInput) -> Result<String, String> {
    if input.prompt.trim().is_empty() {
        return Err("Prompt cannot be empty.".to_string());
    }
    if input.prompt.len() > MAX_PROMPT_LEN {
        return Err(format!(
            "Prompt exceeds maximum length of {MAX_PROMPT_LEN} characters."
        ));
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
        validate_input_path(path, &state.config.output_dir)
            .await
            .map_err(|e| e.to_string())?;
    }
    if let Some(ref mask) = input.mask_path {
        crate::sandbox::validate_mask_path(mask, &state.config.output_dir)
            .await
            .map_err(|e| e.to_string())?;
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
    let format = match &input.output_format {
        Some(f) => parse_format(f).map_err(|e| e.to_string())?,
        None => crate::types::OutputFormat::default(),
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

    validate_request_options(RequestOptions {
        model: &model,
        size: size.as_ref(),
        quality: quality.as_ref(),
        format: &format,
        background: background.as_ref(),
        style: None,
        n,
        output_compression,
        is_edit: true,
    })
    .map_err(|e| e.to_string())?;

    // Create the job in Queued state; a background task will run it.
    let job_id = state
        .job_registry
        .create_job(JobKind::Edit, provider_name, &model, &input.prompt)
        .await;

    info!(job_id = %job_id, model = %model, "edit_image job queued");

    // Build edit request
    let request = EditRequest {
        prompt: input.prompt,
        image_paths: image_paths.clone(),
        mask_path: input.mask_path,
        model: Some(model),
        size,
        quality,
        format: Some(format),
        n: Some(n),
        output_compression,
        background,
        moderation,
        user: input.user,
    };

    // Reserve the session up front (marked in_flight) so the job_id/session_id
    // pair is returned immediately, but continue_edit_session cannot race on
    // last_image_path until the background task finishes.
    let session_id = Uuid::new_v4().to_string();
    state
        .begin_edit_session(&session_id, &input.image_path)
        .await;

    let task_state = state.clone();
    let task_job_id = job_id.clone();
    let task_session_id = session_id.clone();
    let fallback_path = input.image_path.clone();
    state.job_tasks.spawn(async move {
        run_edit_job(
            task_state,
            task_job_id,
            task_session_id,
            request,
            fallback_path,
        )
        .await;
    });

    let output = EditImageOutput {
        job_id,
        session_id: Some(session_id),
        status: "queued".to_string(),
    };

    serde_json::to_string(&output).map_err(|e| format!("Serialization error: {e}"))
}

/// Run an edit job in the background: acquire a concurrency permit, call the
/// provider, save artifacts, update the job and edit session, and record the
/// final status.
async fn run_edit_job(
    state: AppState,
    job_id: String,
    session_id: String,
    request: EditRequest,
    fallback_image_path: String,
) {
    let _permit = match state.job_semaphore.clone().acquire_owned().await {
        Ok(permit) => permit,
        Err(_) => {
            let _ = state
                .job_registry
                .fail_job(&job_id, "Internal error: job scheduler unavailable".into())
                .await;
            state.clear_edit_session_in_flight(&session_id).await;
            return;
        }
    };

    if state
        .job_registry
        .update_status(&job_id, crate::jobs::JobStatus::Running)
        .await
        .is_err()
    {
        state.clear_edit_session_in_flight(&session_id).await;
        return;
    }

    // update_status no-ops on a job already in a terminal state (e.g. expired
    // by the housekeeping worker while queued for a permit). Bail without
    // calling the provider if that happened.
    match state.job_registry.get_job(&job_id).await {
        Ok(job) if job.status != crate::jobs::JobStatus::Running => {
            warn!(job_id = %job_id, status = ?job.status, "edit_image job expired before it could run");
            state.clear_edit_session_in_flight(&session_id).await;
            return;
        }
        Err(_) => {
            state.clear_edit_session_in_flight(&session_id).await;
            return;
        }
        _ => {}
    }

    let result = state.provider.edit(&request).await;
    let fmt = crate::types::OutputFormat::default();
    match result {
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
                    let last = results
                        .last()
                        .map(|r| r.file_path.clone())
                        .unwrap_or(fallback_image_path);
                    info!(job_id = %job_id, count = results.len(), "edit_image job completed");
                    let _ = state.job_registry.complete_job(&job_id, results).await;
                    state.upsert_edit_session(&session_id, &last).await;
                }
                Err(e) => {
                    warn!(job_id = %job_id, error = %e, "edit_image artifact save failed");
                    let _ = state.job_registry.fail_job(&job_id, e.to_string()).await;
                    // The edit failed: leave the session pointing at the original
                    // image and clear in_flight so continue_edit_session works again.
                    state.clear_edit_session_in_flight(&session_id).await;
                }
            }
        }
        Err(e) => {
            warn!(job_id = %job_id, error = %e, "edit_image provider call failed");
            let _ = state.job_registry.fail_job(&job_id, e.to_string()).await;
            state.clear_edit_session_in_flight(&session_id).await;
        }
    }
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
        let state = test_state();
        let state_output = std::path::Path::new(&state.config.output_dir);
        tokio::fs::create_dir_all(state_output).await.unwrap();
        let tmp_image = state_output.join("imagen-edit-test-img.png");
        tokio::fs::write(&tmp_image, b"fake image data")
            .await
            .unwrap();

        let mut input = make_input(&tmp_image.to_string_lossy(), "Edit this");
        input.mask_path = Some(
            state_output
                .join("nonexistent-imagen-test-mask-xyz.png")
                .to_string_lossy()
                .to_string(),
        );
        let err = run(&state, input).await.unwrap_err();
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

    #[tokio::test]
    async fn test_successful_edit_completes_async_and_updates_session() {
        let state = test_state();
        let state_output = std::path::Path::new(&state.config.output_dir);
        tokio::fs::create_dir_all(state_output).await.unwrap();
        let tmp_image = state_output.join("imagen-edit-async-test-img.png");
        tokio::fs::write(&tmp_image, b"fake image data")
            .await
            .unwrap();

        let input = make_input(&tmp_image.to_string_lossy(), "Edit this");
        let output = run(&state, input).await.unwrap();
        assert!(output.contains("\"status\":\"queued\""));

        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        let job_id = parsed["job_id"].as_str().unwrap().to_string();
        let session_id = parsed["session_id"].as_str().unwrap().to_string();

        // Job should reach Completed asynchronously.
        let job = wait_for_terminal_job(&state, &job_id).await;
        assert_eq!(job.status, crate::jobs::JobStatus::Completed);

        // Session should no longer be in_flight and should have a fresh image path.
        let session = state.get_edit_session(&session_id).await.unwrap();
        assert!(!session.in_flight);
        assert_eq!(session.step_count, 1);
        assert_ne!(session.last_image_path, tmp_image.to_string_lossy());

        let _ = tokio::fs::remove_file(&tmp_image).await;
    }

    async fn wait_for_terminal_job(state: &AppState, job_id: &str) -> crate::jobs::Job {
        use crate::jobs::JobStatus;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let job = state.job_registry.get_job(job_id).await.unwrap();
            if matches!(
                job.status,
                JobStatus::Completed | JobStatus::Failed | JobStatus::Expired
            ) {
                return job;
            }
            if tokio::time::Instant::now() >= deadline {
                panic!(
                    "job {job_id} did not reach a terminal state in time (status: {:?})",
                    job.status
                );
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
}
