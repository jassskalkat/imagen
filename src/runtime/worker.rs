use tokio::time::{self, Duration};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, instrument, warn};

use crate::runtime::state::AppState;

/// Background worker that runs periodic maintenance on the job registry and edit sessions.
///
/// Tool handlers (`generate_image`, `edit_image`, `continue_edit_session`) create jobs in
/// `Queued` state and spawn a background task that acquires a concurrency permit,
/// transitions the job to `Running`, calls the provider, and records the final result.
/// The worker's job here is housekeeping only: evicting old terminal jobs, expiring jobs
/// that have been stuck for too long, and cleaning up idle edit sessions. It never runs
/// job work itself.
pub struct Worker {
    state: AppState,
    poll_interval: Duration,
    cancel_token: CancellationToken,
}

impl Worker {
    /// Create a new Worker with the given state and cancellation token.
    pub fn new(state: AppState, cancel_token: CancellationToken) -> Self {
        Self {
            state,
            poll_interval: Duration::from_millis(500),
            cancel_token,
        }
    }

    /// Spawn the worker as a background task.
    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            self.run().await;
        })
    }

    #[instrument(skip(self), name = "worker_loop")]
    async fn run(&self) {
        info!("Worker started");
        loop {
            // Evict terminal jobs older than 1 hour to prevent unbounded memory growth.
            let evicted = self.state.job_registry.evict_terminal_jobs(3600).await;
            if !evicted.is_empty() {
                debug!(count = evicted.len(), "Evicted old terminal jobs");
            }

            // Expire jobs stuck in non-terminal state for over 10 minutes.
            let stale = self.state.job_registry.expire_stale_jobs(600).await;
            if !stale.is_empty() {
                warn!(count = stale.len(), "Expired stale jobs");
            }

            // Expire edit sessions idle for more than 30 minutes.
            let sessions = self.state.expire_edit_sessions(1800).await;
            if !sessions.is_empty() {
                debug!(count = sessions.len(), "Expired old edit sessions");
            }

            tokio::select! {
                _ = time::sleep(self.poll_interval) => {}
                _ = self.cancel_token.cancelled() => {
                    info!("Worker stopped");
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::JobStatus;
    use crate::test_utils::mock_state;

    #[test]
    fn test_worker_creation() {
        let state = mock_state("imagen-worker-test");
        let token = CancellationToken::new();
        let worker = Worker::new(state, token);
        assert_eq!(worker.poll_interval, Duration::from_millis(500));
    }

    #[tokio::test]
    async fn test_worker_stops_on_cancellation() {
        let state = mock_state("imagen-worker-test");
        let token = CancellationToken::new();
        let worker = Worker::new(state, token.clone());
        let handle = worker.spawn();

        token.cancel();

        let result = tokio::time::timeout(Duration::from_secs(5), handle).await;
        assert!(
            result.is_ok(),
            "Worker should have stopped after cancellation"
        );
    }

    #[tokio::test]
    async fn test_worker_evicts_old_jobs() {
        let state = mock_state("imagen-worker-test");

        // Create a completed job — the worker should eventually evict it.
        // Using `create_job_running` here is a deliberate test-only shortcut to
        // skip straight to `Running` before marking it complete; it is not
        // exercising the production job creation path, which is `create_job`
        // (starts `Queued`) followed by `update_status` once a permit is acquired.
        let job_id = state
            .job_registry
            .create_job_running(
                crate::jobs::JobKind::Generate,
                "mock",
                "gpt-image-2",
                "eviction test",
            )
            .await;
        state
            .job_registry
            .complete_job(&job_id, vec![])
            .await
            .unwrap();

        // Run maintenance directly (not through the worker loop) to keep the test fast.
        let evicted = state.job_registry.evict_terminal_jobs(0).await;
        assert!(evicted.contains(&job_id));
        assert!(state.job_registry.get_job(&job_id).await.is_err());
    }

    #[tokio::test]
    async fn test_worker_expires_stale_jobs() {
        let state = mock_state("imagen-worker-test");

        let job_id = state
            .job_registry
            .create_job(
                crate::jobs::JobKind::Generate,
                "mock",
                "gpt-image-2",
                "stale test",
            )
            .await;

        let expired = state.job_registry.expire_stale_jobs(0).await;
        assert!(expired.contains(&job_id));
        assert_eq!(
            state.job_registry.get_job(&job_id).await.unwrap().status,
            JobStatus::Expired
        );
    }
}
