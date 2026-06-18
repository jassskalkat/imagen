use std::sync::Arc;

use tokio::sync::{Mutex, Semaphore};
use tokio::task::JoinHandle;
use tokio::time::{self, Duration};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, instrument, warn};

use crate::artifacts;
use crate::jobs::{JobKind, JobStatus};
use crate::runtime::state::AppState;
use crate::types::{GenerateRequest, ImageResult, OutputFormat};

/// Background worker that polls for queued image generation jobs and processes them.
pub struct Worker {
    state: AppState,
    semaphore: Arc<Semaphore>,
    poll_interval: Duration,
    cancel_token: CancellationToken,
    /// Tracks spawned job task handles so we can await them on shutdown.
    job_handles: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

impl Worker {
    /// Create a new Worker with the given state and cancellation token.
    pub fn new(state: AppState, cancel_token: CancellationToken) -> Self {
        let max_concurrent = state.config.max_concurrent_jobs;
        Self {
            state,
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            poll_interval: Duration::from_millis(500),
            cancel_token,
            job_handles: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Spawn the worker as a background task that continuously polls for jobs.
    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            self.run().await;
        })
    }

    /// Run the worker loop, polling for queued jobs and processing them.
    #[instrument(skip(self), name = "worker_loop")]
    async fn run(&self) {
        info!("Worker started, polling for jobs");
        loop {
            // Find queued jobs
            let queued_jobs = self
                .state
                .job_registry
                .list_jobs(Some(&JobStatus::Queued))
                .await;

            for job in queued_jobs {
                // Skip edit jobs - they are handled inline by tool handlers
                // because they require image file paths that are not stored on
                // the Job struct.
                if matches!(job.kind, JobKind::Edit) {
                    debug!(job_id = %job.id, "Skipping edit job (handled by tool handler)");
                    continue;
                }

                // Try to acquire a permit from the semaphore
                let permit = match self.semaphore.clone().try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => {
                        debug!("Concurrency limit reached, waiting for next poll");
                        break;
                    }
                };

                // Mark the job as running
                if let Err(e) = self
                    .state
                    .job_registry
                    .update_status(&job.id, JobStatus::Running)
                    .await
                {
                    warn!(job_id = %job.id, error = %e, "Failed to update job status");
                    drop(permit);
                    continue;
                }

                let state = self.state.clone();
                let job_id = job.id.clone();
                let job_handles = self.job_handles.clone();

                // Spawn a task to process this job and track its handle
                let handle = tokio::spawn(async move {
                    let _permit = permit; // Hold permit until done

                    debug!(job_id = %job_id, "Processing generate job");

                    let result = process_generate_job(&state, &job_id, &job.prompt).await;

                    match result {
                        Ok(results) => {
                            if let Err(e) = state.job_registry.complete_job(&job_id, results).await
                            {
                                error!(
                                    job_id = %job_id,
                                    error = %e,
                                    "Failed to mark job as completed"
                                );
                            } else {
                                info!(job_id = %job_id, "Job completed successfully");
                            }
                        }
                        Err(e) => {
                            error!(job_id = %job_id, error = %e, "Job failed");
                            if let Err(err) =
                                state.job_registry.fail_job(&job_id, e.to_string()).await
                            {
                                error!(
                                    job_id = %job_id,
                                    error = %err,
                                    "Failed to mark job as failed"
                                );
                            }
                        }
                    }
                });

                job_handles.lock().await.push(handle);
            }

            // Periodic maintenance: evict terminal jobs >1h, expire stale jobs >10min,
            // and expire edit sessions >30min.
            let evicted = self.state.job_registry.evict_terminal_jobs(3600).await;
            if !evicted.is_empty() {
                debug!(count = evicted.len(), "Evicted old terminal jobs");
            }
            let stale = self.state.job_registry.expire_stale_jobs(600).await;
            if !stale.is_empty() {
                warn!(count = stale.len(), "Expired stale jobs");
            }
            let sessions = self.state.expire_edit_sessions(1800).await;
            if !sessions.is_empty() {
                debug!(count = sessions.len(), "Expired old edit sessions");
            }

            // Clean up finished handles to prevent unbounded growth
            {
                let mut handles = self.job_handles.lock().await;
                handles.retain(|h| !h.is_finished());
            }

            // Wait for the poll interval or until cancellation is requested.
            tokio::select! {
                _ = time::sleep(self.poll_interval) => {}
                _ = self.cancel_token.cancelled() => {
                    info!("Worker shutting down, awaiting in-flight jobs...");
                    // Await all in-flight job handles with a timeout
                    let handles: Vec<_> = {
                        let mut locked = self.job_handles.lock().await;
                        locked.drain(..).collect()
                    };
                    let shutdown_timeout = Duration::from_secs(30);
                    let _ = time::timeout(shutdown_timeout, async {
                        for handle in handles {
                            let _ = handle.await;
                        }
                    })
                    .await;
                    info!("Worker stopped");
                    break;
                }
            }
        }
    }
}

/// Process a generation job by calling the provider and saving artifacts.
async fn process_generate_job(
    state: &AppState,
    job_id: &str,
    prompt: &str,
) -> crate::error::Result<Vec<ImageResult>> {
    let request = GenerateRequest {
        prompt: prompt.to_string(),
        model: Some(state.config.default_model.clone()),
        size: None,
        quality: None,
        format: Some(OutputFormat::Png),
        style: None,
        n: Some(1),
        output_compression: None,
        background: None,
        moderation: None,
        user: None,
    };

    let response = state.provider.generate(&request).await?;
    artifacts::save_provider_response(&state.config.output_dir, job_id, &response, &OutputFormat::Png).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::mock_state;

    #[test]
    fn test_worker_creation() {
        let state = mock_state("imagen-worker-test");
        let token = CancellationToken::new();
        let worker = Worker::new(state.clone(), token);
        assert_eq!(worker.poll_interval, Duration::from_millis(500));
    }

    #[tokio::test]
    async fn test_process_generate_job() {
        let state = mock_state("imagen-worker-test");
        let job_id = state
            .job_registry
            .create_job(
                crate::jobs::JobKind::Generate,
                "mock",
                "gpt-image-2",
                "A test prompt",
            )
            .await;

        let results = process_generate_job(&state, &job_id, "A test prompt")
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].revised_prompt, Some("A test image".to_string()));
        assert!(results[0].file_path.ends_with(".png"));

        let _ =
            tokio::fs::remove_dir_all(std::path::Path::new(&state.config.output_dir).join(&job_id))
                .await;
    }

    #[tokio::test]
    async fn test_worker_skips_edit_jobs() {
        let state = mock_state("imagen-worker-test");
        let edit_job_id = state
            .job_registry
            .create_job(
                crate::jobs::JobKind::Edit,
                "mock",
                "gpt-image-2",
                "Edit prompt",
            )
            .await;

        let token = CancellationToken::new();
        let worker = Worker::new(state.clone(), token.clone());
        let handle = worker.spawn();
        tokio::time::sleep(Duration::from_secs(2)).await;

        let job = state.job_registry.get_job(&edit_job_id).await.unwrap();
        assert_eq!(job.status, JobStatus::Queued);

        token.cancel();
        let _ = handle.await;
    }

    #[tokio::test]
    async fn test_worker_processes_queued_job() {
        let state = mock_state("imagen-worker-test");
        let job_id = state
            .job_registry
            .create_job(
                crate::jobs::JobKind::Generate,
                "mock",
                "gpt-image-2",
                "Worker test",
            )
            .await;

        let token = CancellationToken::new();
        let worker = Worker::new(state.clone(), token.clone());
        let handle = worker.spawn();
        tokio::time::sleep(Duration::from_secs(2)).await;

        let job = state.job_registry.get_job(&job_id).await.unwrap();
        assert_eq!(job.status, JobStatus::Completed);
        assert_eq!(job.results.len(), 1);

        token.cancel();
        let _ = handle.await;
        let _ =
            tokio::fs::remove_dir_all(std::path::Path::new(&state.config.output_dir).join(&job_id))
                .await;
    }

    #[tokio::test]
    async fn test_worker_stops_on_cancellation() {
        let state = mock_state("imagen-worker-test");
        let token = CancellationToken::new();
        let worker = Worker::new(state.clone(), token.clone());
        let handle = worker.spawn();

        token.cancel();

        let result = tokio::time::timeout(Duration::from_secs(5), handle).await;
        assert!(
            result.is_ok(),
            "Worker should have stopped after cancellation"
        );
    }
}
