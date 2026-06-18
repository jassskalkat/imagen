use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::jobs::JobStatus;
use crate::runtime::state::AppState;

/// Input parameters for the check_job tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CheckJobInput {
    /// The job ID to check status for.
    pub job_id: String,
}

/// Output from the check_job tool.
#[derive(Debug, Serialize)]
pub struct CheckJobOutput {
    pub job_id: String,
    pub status: String,
    pub kind: String,
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ArtifactInfo>,
}

/// Information about a completed artifact.
#[derive(Debug, Serialize)]
pub struct ArtifactInfo {
    pub file_path: String,
    pub format: String,
    pub size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revised_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base64_preview: Option<String>,
}

/// Maximum file size to include inline base64 preview (512 KB).
const MAX_INLINE_SIZE: u64 = 512 * 1024;

/// Execute the check_job tool logic.
pub async fn run(state: &AppState, input: CheckJobInput) -> Result<String, String> {
    if input.job_id.trim().is_empty() {
        return Err("job_id cannot be empty.".to_string());
    }

    let job = state
        .job_registry
        .get_job(&input.job_id)
        .await
        .map_err(|e| e.to_string())?;

    let status_str = match &job.status {
        JobStatus::Queued => "queued",
        JobStatus::Running => "running",
        JobStatus::Completed => "completed",
        JobStatus::Failed => "failed",
        JobStatus::Expired => "expired",
    };

    let kind_str = serde_json::to_value(&job.kind)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "unknown".to_string());

    let mut artifacts = Vec::new();
    if job.status == JobStatus::Completed {
        for result in &job.results {
            let preview = if result.size_bytes <= MAX_INLINE_SIZE {
                match crate::artifacts::read_artifact(std::path::Path::new(&result.file_path)).await
                {
                    Ok(bytes) => {
                        use base64::Engine;
                        Some(base64::engine::general_purpose::STANDARD.encode(&bytes))
                    }
                    Err(_) => None,
                }
            } else {
                None
            };

            artifacts.push(ArtifactInfo {
                file_path: result.file_path.clone(),
                format: result.format.extension().to_string(),
                size_bytes: result.size_bytes,
                revised_prompt: result.revised_prompt.clone(),
                base64_preview: preview,
            });
        }
    }

    let output = CheckJobOutput {
        job_id: job.id,
        status: status_str.to_string(),
        kind: kind_str,
        prompt: job.prompt,
        error: job.error,
        artifacts,
    };

    serde_json::to_string(&output).map_err(|e| format!("Serialization error: {e}"))
}
