use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::error::{ImagenError, Result};
use crate::types::ImageResult;

/// Status of a job in its lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Expired,
}

/// The kind of request a job represents.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobKind {
    Generate,
    Edit,
}

/// A tracked image generation/editing job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub status: JobStatus,
    pub kind: JobKind,
    pub provider: String,
    pub model: String,
    pub prompt: String,
    pub results: Vec<ImageResult>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// Returns true if the job status is terminal (will not transition further
/// through normal execution).
fn is_terminal(status: &JobStatus) -> bool {
    matches!(
        status,
        JobStatus::Completed | JobStatus::Failed | JobStatus::Expired
    )
}

/// In-memory job registry protected by a read-write lock.
#[derive(Debug, Clone)]
pub struct JobRegistry {
    jobs: Arc<RwLock<HashMap<String, Job>>>,
}

impl JobRegistry {
    /// Create a new empty job registry.
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new job and return its ID.
    ///
    /// The job starts in `Queued` state. Callers spawn a background task that
    /// transitions it to `Running` once it acquires a concurrency permit, then
    /// to `Completed`/`Failed` when the provider call finishes.
    pub async fn create_job(
        &self,
        kind: JobKind,
        provider: &str,
        model: &str,
        prompt: &str,
    ) -> String {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();

        let job = Job {
            id: id.clone(),
            status: JobStatus::Queued,
            kind,
            provider: provider.to_string(),
            model: model.to_string(),
            prompt: prompt.to_string(),
            results: Vec::new(),
            error: None,
            created_at: now,
            updated_at: now,
            completed_at: None,
        };

        let mut jobs = self.jobs.write().await;
        jobs.insert(id.clone(), job);
        id
    }

    /// Create a new job already in `Running` state and return its ID.
    ///
    /// Reserved for callers that acquire their concurrency permit before
    /// creating the job record (so it should never actually be observed in
    /// `Queued` state). Most tool handlers should use [`create_job`] and
    /// transition to `Running` via [`update_status`] once a permit is acquired.
    ///
    /// [`create_job`]: JobRegistry::create_job
    /// [`update_status`]: JobRegistry::update_status
    #[cfg_attr(not(test), allow(dead_code))]
    pub async fn create_job_running(
        &self,
        kind: JobKind,
        provider: &str,
        model: &str,
        prompt: &str,
    ) -> String {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();

        let job = Job {
            id: id.clone(),
            status: JobStatus::Running,
            kind,
            provider: provider.to_string(),
            model: model.to_string(),
            prompt: prompt.to_string(),
            results: Vec::new(),
            error: None,
            created_at: now,
            updated_at: now,
            completed_at: None,
        };

        let mut jobs = self.jobs.write().await;
        jobs.insert(id.clone(), job);
        id
    }

    /// Update a job's status.
    ///
    /// No-ops (returns `Ok`) if the job has already reached a terminal state —
    /// this happens when a background task tries to transition an expired job
    /// to `Running` after the housekeeping worker has already expired it.
    pub async fn update_status(&self, job_id: &str, status: JobStatus) -> Result<()> {
        let mut jobs = self.jobs.write().await;
        let job = jobs
            .get_mut(job_id)
            .ok_or_else(|| ImagenError::JobNotFound(job_id.to_string()))?;

        if is_terminal(&job.status) {
            return Ok(());
        }

        job.status = status.clone();
        job.updated_at = Utc::now();

        if status == JobStatus::Completed || status == JobStatus::Failed {
            job.completed_at = Some(Utc::now());
        }

        Ok(())
    }

    /// Mark a job as completed with results.
    ///
    /// No-ops if the job has already reached a terminal state (e.g. it was
    /// expired by the housekeeping worker while a background task was still
    /// running the provider call). This prevents a stale task from silently
    /// resurrecting an `Expired`/`Failed` job back to `Completed`.
    pub async fn complete_job(&self, job_id: &str, results: Vec<ImageResult>) -> Result<()> {
        let mut jobs = self.jobs.write().await;
        let job = jobs
            .get_mut(job_id)
            .ok_or_else(|| ImagenError::JobNotFound(job_id.to_string()))?;

        if is_terminal(&job.status) {
            return Ok(());
        }

        job.status = JobStatus::Completed;
        job.results = results;
        job.updated_at = Utc::now();
        job.completed_at = Some(Utc::now());
        Ok(())
    }

    /// Mark a job as failed with an error message.
    ///
    /// No-ops if the job has already reached a terminal state, for the same
    /// reason as [`complete_job`].
    ///
    /// [`complete_job`]: JobRegistry::complete_job
    pub async fn fail_job(&self, job_id: &str, error: String) -> Result<()> {
        let mut jobs = self.jobs.write().await;
        let job = jobs
            .get_mut(job_id)
            .ok_or_else(|| ImagenError::JobNotFound(job_id.to_string()))?;

        if is_terminal(&job.status) {
            return Ok(());
        }

        job.status = JobStatus::Failed;
        job.error = Some(error);
        job.updated_at = Utc::now();
        job.completed_at = Some(Utc::now());
        Ok(())
    }

    /// Get a job by ID.
    pub async fn get_job(&self, job_id: &str) -> Result<Job> {
        let jobs = self.jobs.read().await;
        jobs.get(job_id)
            .cloned()
            .ok_or_else(|| ImagenError::JobNotFound(job_id.to_string()))
    }

    /// List all jobs, optionally filtered by status.
    #[cfg_attr(not(test), allow(dead_code))]
    pub async fn list_jobs(&self, status_filter: Option<&JobStatus>) -> Vec<Job> {
        let jobs = self.jobs.read().await;
        jobs.values()
            .filter(|j| status_filter.map(|s| &j.status == s).unwrap_or(true))
            .cloned()
            .collect()
    }

    /// Expire jobs that have been in a non-terminal state for too long.
    pub async fn expire_stale_jobs(&self, max_age_seconds: i64) -> Vec<String> {
        let mut jobs = self.jobs.write().await;
        let now = Utc::now();
        let mut expired = Vec::new();

        for job in jobs.values_mut() {
            if job.status == JobStatus::Queued || job.status == JobStatus::Running {
                let age = (now - job.updated_at).num_seconds();
                if age >= max_age_seconds {
                    job.status = JobStatus::Expired;
                    job.updated_at = now;
                    job.completed_at = Some(now);
                    expired.push(job.id.clone());
                }
            }
        }

        expired
    }

    /// Evict terminal jobs (Completed, Failed, Expired) older than the given
    /// threshold in seconds. This prevents unbounded memory growth for
    /// long-running server instances.
    pub async fn evict_terminal_jobs(&self, max_age_seconds: i64) -> Vec<String> {
        let mut jobs = self.jobs.write().await;
        let now = Utc::now();
        let mut evicted = Vec::new();

        jobs.retain(|id, job| {
            let is_terminal = matches!(
                job.status,
                JobStatus::Completed | JobStatus::Failed | JobStatus::Expired
            );
            if is_terminal {
                let age = (now - job.updated_at).num_seconds();
                if age >= max_age_seconds {
                    evicted.push(id.clone());
                    return false; // Remove from map
                }
            }
            true // Keep in map
        });

        evicted
    }
}

impl Default for JobRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_and_get_job() {
        let registry = JobRegistry::new();
        let id = registry
            .create_job(JobKind::Generate, "openai", "gpt-image-2", "A sunset")
            .await;
        let job = registry.get_job(&id).await.unwrap();
        assert_eq!(job.status, JobStatus::Queued);
        assert_eq!(job.prompt, "A sunset");
        assert_eq!(job.provider, "openai");
    }

    #[tokio::test]
    async fn test_update_status() {
        let registry = JobRegistry::new();
        let id = registry
            .create_job(JobKind::Generate, "azure", "gpt-image-2", "A cat")
            .await;
        registry
            .update_status(&id, JobStatus::Running)
            .await
            .unwrap();
        let job = registry.get_job(&id).await.unwrap();
        assert_eq!(job.status, JobStatus::Running);
    }

    #[tokio::test]
    async fn test_complete_job() {
        let registry = JobRegistry::new();
        let id = registry
            .create_job(JobKind::Generate, "openai", "gpt-image-2", "A dog")
            .await;
        let results = vec![ImageResult {
            file_path: "/tmp/test.png".into(),
            format: crate::types::OutputFormat::Png,
            size_bytes: 1024,
            revised_prompt: None,
        }];
        registry.complete_job(&id, results).await.unwrap();
        let job = registry.get_job(&id).await.unwrap();
        assert_eq!(job.status, JobStatus::Completed);
        assert_eq!(job.results.len(), 1);
        assert!(job.completed_at.is_some());
    }

    #[tokio::test]
    async fn test_fail_job() {
        let registry = JobRegistry::new();
        let id = registry
            .create_job(JobKind::Edit, "openai", "gpt-image-2", "Fix colors")
            .await;
        registry
            .fail_job(&id, "Provider timeout".into())
            .await
            .unwrap();
        let job = registry.get_job(&id).await.unwrap();
        assert_eq!(job.status, JobStatus::Failed);
        assert_eq!(job.error, Some("Provider timeout".into()));
    }

    #[tokio::test]
    async fn test_job_not_found() {
        let registry = JobRegistry::new();
        assert!(registry.get_job("nonexistent").await.is_err());
    }

    #[tokio::test]
    async fn test_list_jobs_with_filter() {
        let registry = JobRegistry::new();
        registry
            .create_job(JobKind::Generate, "openai", "gpt-image-2", "A")
            .await;
        let id2 = registry
            .create_job(JobKind::Generate, "openai", "gpt-image-2", "B")
            .await;
        registry
            .update_status(&id2, JobStatus::Running)
            .await
            .unwrap();
        assert_eq!(registry.list_jobs(Some(&JobStatus::Queued)).await.len(), 1);
        assert_eq!(registry.list_jobs(Some(&JobStatus::Running)).await.len(), 1);
        assert_eq!(registry.list_jobs(None).await.len(), 2);
    }

    #[tokio::test]
    async fn test_expire_stale_jobs() {
        let registry = JobRegistry::new();
        let id = registry
            .create_job(JobKind::Generate, "openai", "gpt-image-2", "Slow")
            .await;
        let expired = registry.expire_stale_jobs(0).await;
        assert!(expired.contains(&id));
        assert_eq!(
            registry.get_job(&id).await.unwrap().status,
            JobStatus::Expired
        );
    }

    #[tokio::test]
    async fn test_terminal_jobs_not_expired_by_stale_check() {
        let registry = JobRegistry::new();
        let id1 = registry
            .create_job(JobKind::Generate, "openai", "gpt-image-2", "Done")
            .await;
        registry.complete_job(&id1, vec![]).await.unwrap();
        let id2 = registry
            .create_job(JobKind::Generate, "openai", "gpt-image-2", "Fail")
            .await;
        registry.fail_job(&id2, "error".into()).await.unwrap();

        let expired = registry.expire_stale_jobs(0).await;
        assert!(!expired.contains(&id1));
        assert!(!expired.contains(&id2));
        assert_eq!(
            registry.get_job(&id1).await.unwrap().status,
            JobStatus::Completed
        );
        assert_eq!(
            registry.get_job(&id2).await.unwrap().status,
            JobStatus::Failed
        );
    }

    #[tokio::test]
    async fn test_status_transition_full_lifecycle() {
        let registry = JobRegistry::new();
        let id = registry
            .create_job(JobKind::Edit, "azure", "gpt-image-2", "Full cycle")
            .await;
        registry
            .update_status(&id, JobStatus::Running)
            .await
            .unwrap();
        let results = vec![ImageResult {
            file_path: "/tmp/out.png".into(),
            format: crate::types::OutputFormat::Png,
            size_bytes: 2048,
            revised_prompt: Some("Full cycle revised".into()),
        }];
        registry.complete_job(&id, results).await.unwrap();
        let job = registry.get_job(&id).await.unwrap();
        assert_eq!(job.status, JobStatus::Completed);
        assert!(job.completed_at.is_some());
        assert_eq!(
            job.results[0].revised_prompt,
            Some("Full cycle revised".into())
        );
    }

    #[tokio::test]
    async fn test_update_nonexistent_job() {
        let registry = JobRegistry::new();
        assert!(registry
            .update_status("no-such-id", JobStatus::Running)
            .await
            .is_err());
        assert!(registry.fail_job("no-such-id", "err".into()).await.is_err());
        assert!(registry.complete_job("no-such-id", vec![]).await.is_err());
    }

    #[tokio::test]
    async fn test_evict_terminal_jobs() {
        let registry = JobRegistry::new();
        let id = registry
            .create_job(JobKind::Generate, "openai", "gpt-image-2", "Evict me")
            .await;
        registry.complete_job(&id, vec![]).await.unwrap();
        let evicted = registry.evict_terminal_jobs(0).await;
        assert!(evicted.contains(&id));
        assert!(registry.get_job(&id).await.is_err());
    }

    #[tokio::test]
    async fn test_evict_does_not_remove_running_jobs() {
        let registry = JobRegistry::new();
        let id = registry
            .create_job(JobKind::Generate, "openai", "gpt-image-2", "Keep me")
            .await;
        registry
            .update_status(&id, JobStatus::Running)
            .await
            .unwrap();
        let evicted = registry.evict_terminal_jobs(0).await;
        assert!(!evicted.contains(&id));
        assert_eq!(
            registry.get_job(&id).await.unwrap().status,
            JobStatus::Running
        );
    }

    #[tokio::test]
    async fn test_complete_job_does_not_resurrect_expired_job() {
        let registry = JobRegistry::new();
        let id = registry
            .create_job(JobKind::Generate, "openai", "gpt-image-2", "Slow job")
            .await;
        // Simulate the housekeeping worker expiring the job while a background
        // task is still mid-flight on the provider call.
        let expired = registry.expire_stale_jobs(0).await;
        assert!(expired.contains(&id));

        // The background task finishes afterwards and tries to complete it —
        // this must not overwrite the Expired status.
        registry.complete_job(&id, vec![]).await.unwrap();
        let job = registry.get_job(&id).await.unwrap();
        assert_eq!(job.status, JobStatus::Expired);
        assert!(job.results.is_empty());
    }

    #[tokio::test]
    async fn test_fail_job_does_not_resurrect_expired_job() {
        let registry = JobRegistry::new();
        let id = registry
            .create_job(JobKind::Generate, "openai", "gpt-image-2", "Slow job")
            .await;
        let expired = registry.expire_stale_jobs(0).await;
        assert!(expired.contains(&id));

        registry
            .fail_job(&id, "provider error after expiry".into())
            .await
            .unwrap();
        let job = registry.get_job(&id).await.unwrap();
        assert_eq!(job.status, JobStatus::Expired);
        assert!(job.error.is_none());
    }

    #[tokio::test]
    async fn test_update_status_does_not_resurrect_expired_job() {
        let registry = JobRegistry::new();
        let id = registry
            .create_job(JobKind::Generate, "openai", "gpt-image-2", "Slow job")
            .await;
        let expired = registry.expire_stale_jobs(0).await;
        assert!(expired.contains(&id));

        // A background task that acquired its permit late tries to mark the
        // job Running — this must be a no-op, not a resurrection.
        registry
            .update_status(&id, JobStatus::Running)
            .await
            .unwrap();
        let job = registry.get_job(&id).await.unwrap();
        assert_eq!(job.status, JobStatus::Expired);
    }

    #[tokio::test]
    async fn test_complete_job_does_not_overwrite_already_failed_job() {
        let registry = JobRegistry::new();
        let id = registry
            .create_job(JobKind::Generate, "openai", "gpt-image-2", "Racy job")
            .await;
        registry.fail_job(&id, "first error".into()).await.unwrap();

        // A second, stale completion attempt must not clobber the failure.
        registry.complete_job(&id, vec![]).await.unwrap();
        let job = registry.get_job(&id).await.unwrap();
        assert_eq!(job.status, JobStatus::Failed);
        assert_eq!(job.error, Some("first error".into()));
    }
}
