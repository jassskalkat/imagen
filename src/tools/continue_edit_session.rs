use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::{info, instrument, warn};

use crate::error::ImagenError;
use crate::jobs::JobKind;
use crate::runtime::state::AppState;
use crate::sandbox::validate_input_path;
use crate::tools::parse::{parse_background, parse_compression, parse_moderation};
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
    /// Output compression percentage (0-100).
    pub output_compression: Option<u8>,
    /// Image background: "opaque" or "auto".
    pub background: Option<String>,
    /// Content moderation level: "low" or "auto".
    pub moderation: Option<String>,
    /// User tracking identifier passed to the API.
    pub user: Option<String>,
}

/// Output from the continue_edit_session tool.
#[derive(Debug, Serialize)]
pub struct ContinueEditSessionOutput {
    pub job_id: String,
    pub session_id: String,
    pub step: u32,
    pub status: String,
}

/// Maximum prompt length in bytes accepted by the server.
const MAX_PROMPT_LEN: usize = 4000;

/// Execute the continue_edit_session tool logic.
#[instrument(skip(state), fields(session_id = %input.session_id, prompt_len = input.prompt.len()))]
pub async fn run(state: &AppState, input: ContinueEditSessionInput) -> Result<String, String> {
    if input.session_id.trim().is_empty() {
        return Err("session_id cannot be empty.".to_string());
    }
    if input.prompt.trim().is_empty() {
        return Err("Prompt cannot be empty.".to_string());
    }
    if input.prompt.len() > MAX_PROMPT_LEN {
        return Err(format!(
            "Prompt exceeds maximum length of {MAX_PROMPT_LEN} characters."
        ));
    }

    let output_compression = match input.output_compression {
        Some(c) => Some(parse_compression(c).map_err(|e| e.to_string())?),
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

    // Look up session
    let session = state
        .get_edit_session(&input.session_id)
        .await
        .ok_or_else(|| ImagenError::SessionExpired(input.session_id.clone()).to_string())?;

    if session.in_flight {
        return Err(format!(
            "Session '{}' has an edit already in progress. Wait for it to complete before continuing.",
            input.session_id
        ));
    }

    // Validate session's last image still exists and is safe
    validate_input_path(&session.last_image_path)
        .await
        .map_err(|e| format!("Session image no longer valid: {}", e))?;

    // Validate mask file if provided
    if let Some(ref mask) = input.mask_path {
        crate::sandbox::validate_mask_path(mask)
            .await
            .map_err(|e| e.to_string())?;
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

    // Create the job in Queued state; a background task will run it.
    let job_id = state
        .job_registry
        .create_job(JobKind::Edit, provider_name, &model, &input.prompt)
        .await;

    info!(job_id = %job_id, model = %model, step = session.step_count + 1, "continue_edit_session job queued");

    // Mark the session in_flight up front so a concurrent continue_edit_session
    // call against the same session is rejected rather than racing.
    state
        .begin_edit_session(&input.session_id, &session.last_image_path)
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
        output_compression,
        background,
        moderation,
        user: input.user,
    };

    let step = session.step_count + 1;
    let task_state = state.clone();
    let task_job_id = job_id.clone();
    let task_session_id = input.session_id.clone();
    state.job_tasks.spawn(async move {
        run_continue_edit_job(task_state, task_job_id, task_session_id, request).await;
    });

    let output = ContinueEditSessionOutput {
        job_id,
        session_id: input.session_id,
        step,
        status: "queued".to_string(),
    };

    serde_json::to_string(&output).map_err(|e| format!("Serialization error: {e}"))
}

/// Run a continue-edit-session job in the background: acquire a concurrency
/// permit, call the provider, save artifacts, update the job and session,
/// and record the final status.
async fn run_continue_edit_job(
    state: AppState,
    job_id: String,
    session_id: String,
    request: EditRequest,
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

    match state.job_registry.get_job(&job_id).await {
        Ok(job) if job.status != crate::jobs::JobStatus::Running => {
            warn!(job_id = %job_id, status = ?job.status, "continue_edit_session job expired before it could run");
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
                    let last = results.last().map(|r| r.file_path.clone());
                    info!(job_id = %job_id, count = results.len(), "continue_edit_session job completed");
                    let _ = state.job_registry.complete_job(&job_id, results).await;
                    if let Some(last) = last {
                        state.upsert_edit_session(&session_id, &last).await;
                    } else {
                        state.clear_edit_session_in_flight(&session_id).await;
                    }
                }
                Err(e) => {
                    warn!(job_id = %job_id, error = %e, "continue_edit_session artifact save failed");
                    let _ = state.job_registry.fail_job(&job_id, e.to_string()).await;
                    state.clear_edit_session_in_flight(&session_id).await;
                }
            }
        }
        Err(e) => {
            warn!(job_id = %job_id, error = %e, "continue_edit_session provider call failed");
            let _ = state.job_registry.fail_job(&job_id, e.to_string()).await;
            state.clear_edit_session_in_flight(&session_id).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::mock_state;

    #[tokio::test]
    async fn test_empty_session_id_returns_error() {
        let state = mock_state("imagen-continue-test");
        let input = ContinueEditSessionInput {
            session_id: "  ".to_string(),
            prompt: "Next edit".to_string(),
            mask_path: None,
            output_compression: None,
            background: None,
            moderation: None,
            user: None,
        };
        let result = run(&state, input).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("session_id cannot be empty"));
    }

    #[tokio::test]
    async fn test_empty_prompt_returns_error() {
        let state = mock_state("imagen-continue-test");
        let input = ContinueEditSessionInput {
            session_id: "some-session".to_string(),
            prompt: "   ".to_string(),
            mask_path: None,
            output_compression: None,
            background: None,
            moderation: None,
            user: None,
        };
        let result = run(&state, input).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Prompt cannot be empty"));
    }

    #[tokio::test]
    async fn test_nonexistent_session_returns_error() {
        let state = mock_state("imagen-continue-test");
        let input = ContinueEditSessionInput {
            session_id: "nonexistent-session-id".to_string(),
            prompt: "Apply edits".to_string(),
            mask_path: None,
            output_compression: None,
            background: None,
            moderation: None,
            user: None,
        };
        let result = run(&state, input).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("expired") || err.contains("Session expired"),
            "Expected 'expired' error, got: {err}"
        );
    }

    #[tokio::test]
    async fn test_in_flight_session_rejects_concurrent_continue() {
        let state = mock_state("imagen-continue-inflight-test");
        let tmp_image = std::env::temp_dir().join("imagen-continue-inflight-img.png");
        tokio::fs::write(&tmp_image, b"fake image data")
            .await
            .unwrap();

        state
            .begin_edit_session("busy-session", &tmp_image.to_string_lossy())
            .await;

        let input = ContinueEditSessionInput {
            session_id: "busy-session".to_string(),
            prompt: "Apply edits".to_string(),
            mask_path: None,
            output_compression: None,
            background: None,
            moderation: None,
            user: None,
        };
        let result = run(&state, input).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already in progress"));

        let _ = tokio::fs::remove_file(&tmp_image).await;
    }

    #[tokio::test]
    async fn test_successful_continue_completes_async() {
        let state = mock_state("imagen-continue-async-test");
        let tmp_image = std::env::temp_dir().join("imagen-continue-async-img.png");
        tokio::fs::write(&tmp_image, b"fake image data")
            .await
            .unwrap();

        state
            .upsert_edit_session("session-a", &tmp_image.to_string_lossy())
            .await;

        let input = ContinueEditSessionInput {
            session_id: "session-a".to_string(),
            prompt: "Apply more edits".to_string(),
            mask_path: None,
            output_compression: None,
            background: None,
            moderation: None,
            user: None,
        };
        let output = run(&state, input).await.unwrap();
        assert!(output.contains("\"status\":\"queued\""));

        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        let job_id = parsed["job_id"].as_str().unwrap().to_string();

        let job = wait_for_terminal_job(&state, &job_id).await;
        assert_eq!(job.status, crate::jobs::JobStatus::Completed);

        let session = state.get_edit_session("session-a").await.unwrap();
        assert!(!session.in_flight);
        assert_eq!(session.step_count, 2);

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
