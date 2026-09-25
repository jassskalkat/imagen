use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::{info, instrument, warn};

use crate::cost;
use crate::jobs::JobKind;
use crate::providers::{validate_request_options, RequestOptions};
use crate::runtime::state::AppState;
use crate::tools::parse::{
    parse_background, parse_compression, parse_format, parse_moderation, parse_quality, parse_size,
    parse_style,
};
use crate::types::{GenerateRequest, ImageQuality, ImageSize, OutputFormat};

/// Input parameters for the generate_image tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GenerateImageInput {
    /// The text prompt describing the image to generate.
    pub prompt: String,
    /// Image size: "1024x1024", "1536x1024", "1024x1536", "auto", or arbitrary "WxH".
    pub size: Option<String>,
    /// Image quality: "low", "medium", "high", "xhigh", "max", "auto", "standard", or "hd".
    pub quality: Option<String>,
    /// Image style: "vivid" or "natural". Supported only for dall-e-3.
    pub style: Option<String>,
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

/// Output from the generate_image tool.
#[derive(Debug, Serialize)]
pub struct GenerateImageOutput {
    pub job_id: String,
    pub status: String,
    pub cost_estimate: cost::CostEstimate,
}

/// Maximum prompt length in bytes accepted by the server.
const MAX_PROMPT_LEN: usize = 32_000;

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
        Some(s) => Some(parse_style(s).map_err(|e| e.to_string())?),
        None => None,
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

    let provider_name = state.provider.provider_name();
    let model = state.config.default_model.clone();

    validate_request_options(RequestOptions {
        model: &model,
        size: Some(&size),
        quality: Some(&quality),
        format: &format,
        background: background.as_ref(),
        style: style.as_ref(),
        n,
        output_compression,
        is_edit: false,
    })
    .map_err(|e| e.to_string())?;

    // Create the job in Queued state. A background task will acquire a
    // concurrency permit, transition it to Running, call the provider, and
    // record the final result. This call returns immediately.
    let job_id = state
        .job_registry
        .create_job(JobKind::Generate, provider_name, &model, &input.prompt)
        .await;

    info!(job_id = %job_id, model = %model, "generate_image job queued");

    let estimate = cost::estimate_cost(provider_name, &model, &size, &quality, n);

    // Build the provider request
    let request = GenerateRequest {
        prompt: input.prompt,
        model: Some(model.clone()),
        size: Some(size),
        quality: Some(quality),
        format: Some(format.clone()),
        style,
        n: Some(n),
        output_compression,
        background,
        moderation,
        user: input.user,
    };

    let task_state = state.clone();
    let task_job_id = job_id.clone();
    state.job_tasks.spawn(async move {
        run_generate_job(task_state, task_job_id, request, format).await;
    });

    let output = GenerateImageOutput {
        job_id,
        status: "queued".to_string(),
        cost_estimate: estimate,
    };

    serde_json::to_string(&output).map_err(|e| format!("Serialization error: {e}"))
}

/// Run a generate job in the background: acquire a concurrency permit, call
/// the provider, save artifacts, and record the final status on the job.
async fn run_generate_job(
    state: AppState,
    job_id: String,
    request: GenerateRequest,
    format: OutputFormat,
) {
    let _permit = match state.job_semaphore.clone().acquire_owned().await {
        Ok(permit) => permit,
        Err(_) => {
            // Semaphore closed (should not happen; it is never explicitly closed).
            let _ = state
                .job_registry
                .fail_job(&job_id, "Internal error: job scheduler unavailable".into())
                .await;
            return;
        }
    };

    if state
        .job_registry
        .update_status(&job_id, crate::jobs::JobStatus::Running)
        .await
        .is_err()
    {
        // Job vanished entirely (e.g. evicted); nothing to update.
        return;
    }

    // update_status no-ops on a job that already reached a terminal state
    // (e.g. expired by the housekeeping worker while queued for a permit).
    // Re-check the actual status so we don't waste a provider call on a
    // job the client has already given up on.
    match state.job_registry.get_job(&job_id).await {
        Ok(job) if job.status != crate::jobs::JobStatus::Running => {
            warn!(job_id = %job_id, status = ?job.status, "generate_image job expired before it could run");
            return;
        }
        Err(_) => return,
        _ => {}
    }

    let result = state.provider.generate(&request).await;
    match result {
        Ok(response) => {
            match crate::artifacts::save_provider_response(
                &state.config.output_dir,
                &job_id,
                &response,
                &format,
            )
            .await
            {
                Ok(results) => {
                    info!(job_id = %job_id, count = results.len(), "generate_image job completed");
                    let _ = state.job_registry.complete_job(&job_id, results).await;
                }
                Err(e) => {
                    let msg = e.to_string();
                    warn!(job_id = %job_id, error = %msg, "generate_image artifact save failed");
                    let _ = state.job_registry.fail_job(&job_id, msg).await;
                }
            }
        }
        Err(e) => {
            let error_msg = e.to_string();
            warn!(job_id = %job_id, error = %error_msg, "generate_image provider call failed");
            let _ = state.job_registry.fail_job(&job_id, error_msg).await;
        }
    }
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
        assert!(result.is_ok(), "Should return Ok with queued status");
        let output = result.unwrap();
        assert!(
            output.contains("\"status\":\"queued\""),
            "Status should be queued immediately, got: {output}"
        );

        let job_id: serde_json::Value = serde_json::from_str(&output).unwrap();
        let job_id = job_id["job_id"].as_str().unwrap().to_string();

        // The background task runs asynchronously; poll briefly for the
        // terminal Failed status instead of asserting immediately.
        let job = wait_for_terminal_job(&state, &job_id).await;
        assert_eq!(job.status, crate::jobs::JobStatus::Failed);
        assert!(job.error.is_some());
    }

    /// Poll the job registry until the job reaches a terminal state or a
    /// timeout elapses. Used because job execution is now a spawned task.
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

    #[tokio::test]
    async fn test_generate_job_expired_before_running_stays_expired() {
        // Use a semaphore with zero available permits by holding the only one,
        // so the background task cannot proceed to Running before the job is
        // expired out from under it.
        let state = test_state();
        let _permit = state.job_semaphore.clone().acquire_owned().await.unwrap();

        let output = run(&state, make_input("A cat")).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        let job_id = parsed["job_id"].as_str().unwrap().to_string();

        // The background task is blocked waiting for a permit. Expire the job
        // out from under it, simulating the housekeeping worker.
        let expired = state.job_registry.expire_stale_jobs(0).await;
        assert!(expired.contains(&job_id));

        // Release the permit so the background task can proceed and observe
        // the job is no longer Running-eligible.
        drop(_permit);

        // Give the background task a moment to wake up, acquire the permit,
        // and discover the job is already terminal.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let job = state.job_registry.get_job(&job_id).await.unwrap();
        assert_eq!(
            job.status,
            crate::jobs::JobStatus::Expired,
            "job must remain Expired, not be resurrected to Completed/Failed"
        );
    }
}
