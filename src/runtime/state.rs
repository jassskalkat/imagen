use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, Semaphore};
use tokio_util::task::TaskTracker;

use crate::config::AppConfig;
use crate::jobs::JobRegistry;
use crate::providers::ImageProvider;

/// An active multi-turn edit session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditSession {
    pub session_id: String,
    pub last_image_path: String,
    pub step_count: u32,
    pub last_updated: DateTime<Utc>,
    pub provider: String,
    /// True while a background edit task is running against this session.
    /// Prevents a second `continue_edit_session` call from racing on a
    /// `last_image_path` that is about to be overwritten.
    #[serde(default)]
    pub in_flight: bool,
}

/// Shared application state accessible by all tool handlers and the worker.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub job_registry: Arc<JobRegistry>,
    pub provider: Arc<dyn ImageProvider>,
    pub edit_sessions: Arc<RwLock<HashMap<String, EditSession>>>,
    /// Bounds the number of provider calls (generate/edit) running concurrently,
    /// enforcing `config.max_concurrent_jobs`. Background job tasks acquire a
    /// permit before calling the provider and hold it for the duration of the call.
    pub job_semaphore: Arc<Semaphore>,
    /// Tracks all spawned background job tasks (generate/edit/continue) so the
    /// server can wait for in-flight work to finish during graceful shutdown
    /// instead of the tokio runtime silently dropping them.
    pub job_tasks: TaskTracker,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("config", &self.config)
            .field("provider", &self.provider.provider_name())
            .finish_non_exhaustive()
    }
}

impl AppState {
    /// Create a new AppState with the given config and provider.
    pub fn new(config: AppConfig, provider: Arc<dyn ImageProvider>) -> Self {
        let max_concurrent_jobs = config.max_concurrent_jobs.max(1);
        Self {
            config: Arc::new(config),
            job_registry: Arc::new(JobRegistry::new()),
            provider,
            edit_sessions: Arc::new(RwLock::new(HashMap::new())),
            job_semaphore: Arc::new(Semaphore::new(max_concurrent_jobs)),
            job_tasks: TaskTracker::new(),
        }
    }

    /// Reserve a session slot before spawning a background edit task, marking
    /// it `in_flight` so concurrent `continue_edit_session` calls are rejected
    /// until the task completes. Creates the session if it does not exist yet.
    pub async fn begin_edit_session(&self, session_id: &str, image_path: &str) {
        let mut sessions = self.edit_sessions.write().await;
        let entry = sessions
            .entry(session_id.to_string())
            .or_insert_with(|| EditSession {
                session_id: session_id.to_string(),
                last_image_path: image_path.to_string(),
                step_count: 0,
                last_updated: Utc::now(),
                provider: self.provider.provider_name().to_string(),
                in_flight: false,
            });
        entry.in_flight = true;
        entry.last_updated = Utc::now();
    }

    /// Create or update an edit session with the result of a completed edit,
    /// clearing the `in_flight` flag.
    pub async fn upsert_edit_session(&self, session_id: &str, image_path: &str) {
        let mut sessions = self.edit_sessions.write().await;
        let entry = sessions
            .entry(session_id.to_string())
            .or_insert_with(|| EditSession {
                session_id: session_id.to_string(),
                last_image_path: image_path.to_string(),
                step_count: 0,
                last_updated: Utc::now(),
                provider: self.provider.provider_name().to_string(),
                in_flight: false,
            });
        entry.last_image_path = image_path.to_string();
        entry.step_count += 1;
        entry.last_updated = Utc::now();
        entry.in_flight = false;
    }

    /// Clear the `in_flight` flag on a session without changing its image path
    /// or step count. Used when a background edit task fails.
    pub async fn clear_edit_session_in_flight(&self, session_id: &str) {
        let mut sessions = self.edit_sessions.write().await;
        if let Some(entry) = sessions.get_mut(session_id) {
            entry.in_flight = false;
            entry.last_updated = Utc::now();
        }
    }

    /// Get an edit session by ID.
    pub async fn get_edit_session(&self, session_id: &str) -> Option<EditSession> {
        let sessions = self.edit_sessions.read().await;
        sessions.get(session_id).cloned()
    }

    /// Remove expired edit sessions (older than the specified duration in seconds).
    pub async fn expire_edit_sessions(&self, max_age_seconds: i64) -> Vec<String> {
        let mut sessions = self.edit_sessions.write().await;
        let now = Utc::now();
        let mut expired = Vec::new();

        sessions.retain(|id, session| {
            let age = (now - session.last_updated).num_seconds();
            if age >= max_age_seconds {
                expired.push(id.clone());
                false
            } else {
                true
            }
        });

        expired
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{mock_config, MockProvider};
    use std::sync::Arc;

    fn test_state() -> AppState {
        AppState::new(mock_config("/tmp/test"), Arc::new(MockProvider))
    }

    #[tokio::test]
    async fn test_app_state_creation() {
        let state = test_state();
        assert_eq!(state.config.max_concurrent_jobs, 2);
        assert_eq!(state.provider.provider_name(), "mock");
    }

    #[tokio::test]
    async fn test_edit_session_lifecycle() {
        let state = test_state();

        state
            .upsert_edit_session("session-1", "/tmp/image.png")
            .await;

        let session = state.get_edit_session("session-1").await.unwrap();
        assert_eq!(session.step_count, 1);
        assert_eq!(session.last_image_path, "/tmp/image.png");
        assert_eq!(session.provider, "mock");

        state
            .upsert_edit_session("session-1", "/tmp/image2.png")
            .await;
        let session = state.get_edit_session("session-1").await.unwrap();
        assert_eq!(session.step_count, 2);
        assert_eq!(session.last_image_path, "/tmp/image2.png");
    }

    #[tokio::test]
    async fn test_expire_edit_sessions() {
        let state = test_state();

        state
            .upsert_edit_session("session-1", "/tmp/image.png")
            .await;

        let expired = state.expire_edit_sessions(0).await;
        assert!(expired.contains(&"session-1".to_string()));
        assert!(state.get_edit_session("session-1").await.is_none());
    }

    #[tokio::test]
    async fn test_nonexistent_session() {
        let state = test_state();
        assert!(state.get_edit_session("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn test_job_semaphore_sized_from_config() {
        let state = test_state();
        // mock_config sets max_concurrent_jobs = 2
        assert_eq!(state.job_semaphore.available_permits(), 2);
    }

    #[tokio::test]
    async fn test_job_semaphore_minimum_one_permit() {
        let mut config = mock_config("/tmp/test-zero-permits");
        config.max_concurrent_jobs = 0;
        let state = AppState::new(config, Arc::new(MockProvider));
        assert_eq!(state.job_semaphore.available_permits(), 1);
    }

    #[tokio::test]
    async fn test_begin_edit_session_marks_in_flight() {
        let state = test_state();
        state
            .begin_edit_session("session-1", "/tmp/image.png")
            .await;
        let session = state.get_edit_session("session-1").await.unwrap();
        assert!(session.in_flight);
        assert_eq!(session.step_count, 0);
    }

    #[tokio::test]
    async fn test_upsert_edit_session_clears_in_flight() {
        let state = test_state();
        state
            .begin_edit_session("session-1", "/tmp/image.png")
            .await;
        state
            .upsert_edit_session("session-1", "/tmp/image2.png")
            .await;
        let session = state.get_edit_session("session-1").await.unwrap();
        assert!(!session.in_flight);
        assert_eq!(session.step_count, 1);
        assert_eq!(session.last_image_path, "/tmp/image2.png");
    }

    #[tokio::test]
    async fn test_clear_edit_session_in_flight_on_failure() {
        let state = test_state();
        state
            .begin_edit_session("session-1", "/tmp/image.png")
            .await;
        state.clear_edit_session_in_flight("session-1").await;
        let session = state.get_edit_session("session-1").await.unwrap();
        assert!(!session.in_flight);
        // Image path/step count unchanged from the failed attempt.
        assert_eq!(session.last_image_path, "/tmp/image.png");
        assert_eq!(session.step_count, 0);
    }
}
