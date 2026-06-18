use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

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
}

/// Shared application state accessible by all tool handlers and the worker.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub job_registry: Arc<JobRegistry>,
    pub provider: Arc<dyn ImageProvider>,
    pub edit_sessions: Arc<RwLock<HashMap<String, EditSession>>>,
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
    pub fn new(
        config: AppConfig,
        provider: Arc<dyn ImageProvider>,
    ) -> Self {
        Self {
            config: Arc::new(config),
            job_registry: Arc::new(JobRegistry::new()),
            provider,
            edit_sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create or update an edit session.
    pub async fn upsert_edit_session(
        &self,
        session_id: &str,
        image_path: &str,
    ) {
        let mut sessions = self.edit_sessions.write().await;
        let entry = sessions
            .entry(session_id.to_string())
            .or_insert_with(|| EditSession {
                session_id: session_id.to_string(),
                last_image_path: image_path.to_string(),
                step_count: 0,
                last_updated: Utc::now(),
                provider: self.provider.provider_name().to_string(),
            });
        entry.last_image_path = image_path.to_string();
        entry.step_count += 1;
        entry.last_updated = Utc::now();
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
    use crate::config::Provider;
    use crate::error::Result;
    use crate::providers::ModelInfo;
    use crate::types::{EditRequest, GenerateRequest, ProviderResponse};
    use async_trait::async_trait;

    /// A mock provider for testing.
    struct MockProvider;

    #[async_trait]
    impl ImageProvider for MockProvider {
        async fn generate(&self, _request: &GenerateRequest) -> Result<ProviderResponse> {
            Ok(ProviderResponse {
                images: vec![],
                model: "mock".to_string(),
                usage: None,
            })
        }

        async fn edit(&self, _request: &EditRequest) -> Result<ProviderResponse> {
            Ok(ProviderResponse {
                images: vec![],
                model: "mock".to_string(),
                usage: None,
            })
        }

        fn get_models(&self) -> Vec<ModelInfo> {
            vec![ModelInfo {
                id: "mock-model".to_string(),
                name: "Mock Model".to_string(),
                supports_editing: true,
                max_images: 4,
            }]
        }

        fn provider_name(&self) -> &'static str {
            "mock"
        }
    }

    fn test_config() -> AppConfig {
        AppConfig {
            provider: Provider::OpenAI,
            azure_endpoint: None,
            azure_deployment_name: None,
            azure_api_key: None,
            azure_api_version: None,
            openai_api_key: Some("sk-test".into()),
            openai_org_id: None,
            output_dir: "/tmp/test".into(),
            max_concurrent_jobs: 4,
            default_model: "gpt-image-2".into(),
        }
    }

    #[tokio::test]
    async fn test_app_state_creation() {
        let config = test_config();
        let provider = Arc::new(MockProvider);
        let state = AppState::new(config, provider);

        assert_eq!(state.config.max_concurrent_jobs, 4);
        assert_eq!(state.provider.provider_name(), "mock");
    }

    #[tokio::test]
    async fn test_edit_session_lifecycle() {
        let config = test_config();
        let provider = Arc::new(MockProvider);
        let state = AppState::new(config, provider);

        // Create session
        state
            .upsert_edit_session("session-1", "/tmp/image.png")
            .await;

        // Get session
        let session = state.get_edit_session("session-1").await.unwrap();
        assert_eq!(session.step_count, 1);
        assert_eq!(session.last_image_path, "/tmp/image.png");
        assert_eq!(session.provider, "mock");

        // Update session
        state
            .upsert_edit_session("session-1", "/tmp/image2.png")
            .await;
        let session = state.get_edit_session("session-1").await.unwrap();
        assert_eq!(session.step_count, 2);
        assert_eq!(session.last_image_path, "/tmp/image2.png");
    }

    #[tokio::test]
    async fn test_expire_edit_sessions() {
        let config = test_config();
        let provider = Arc::new(MockProvider);
        let state = AppState::new(config, provider);

        state
            .upsert_edit_session("session-1", "/tmp/image.png")
            .await;

        // With max_age of 0, it should expire immediately
        let expired = state.expire_edit_sessions(0).await;
        assert!(expired.contains(&"session-1".to_string()));

        // Session should be gone
        assert!(state.get_edit_session("session-1").await.is_none());
    }

    #[tokio::test]
    async fn test_nonexistent_session() {
        let config = test_config();
        let provider = Arc::new(MockProvider);
        let state = AppState::new(config, provider);

        assert!(state.get_edit_session("nonexistent").await.is_none());
    }
}
