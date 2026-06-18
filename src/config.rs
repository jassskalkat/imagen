use crate::error::{ImagenError, Result};
use serde::{Deserialize, Serialize};

/// Which provider to use for image generation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Azure,
    OpenAI,
}

/// Application configuration loaded from environment variables.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub provider: Provider,
    pub azure_endpoint: Option<String>,
    pub azure_deployment_name: Option<String>,
    pub azure_api_key: Option<String>,
    pub azure_api_version: Option<String>,
    pub openai_api_key: Option<String>,
    pub openai_org_id: Option<String>,
    pub output_dir: String,
    pub max_concurrent_jobs: usize,
    pub default_model: String,
}

impl AppConfig {
    /// Load configuration from environment variables.
    pub fn from_env() -> Result<Self> {
        let provider = match std::env::var("IMAGEN_PROVIDER")
            .unwrap_or_else(|_| "openai".into())
            .to_lowercase()
            .as_str()
        {
            "azure" => Provider::Azure,
            "openai" => Provider::OpenAI,
            other => {
                return Err(ImagenError::ConfigError(format!(
                    "Unknown provider: {other}. Expected 'azure' or 'openai'."
                )));
            }
        };

        let output_dir = std::env::var("IMAGEN_OUTPUT_DIR")
            .unwrap_or_else(|_| "./imagen-output".into());

        let max_concurrent_jobs: usize = std::env::var("IMAGEN_MAX_CONCURRENT_JOBS")
            .unwrap_or_else(|_| "4".into())
            .parse()
            .map_err(|e| {
                ImagenError::ConfigError(format!("Invalid IMAGEN_MAX_CONCURRENT_JOBS: {e}"))
            })?;

        let default_model = std::env::var("IMAGEN_DEFAULT_MODEL")
            .unwrap_or_else(|_| "gpt-image-2".into());

        let config = AppConfig {
            provider,
            azure_endpoint: std::env::var("AZURE_OPENAI_ENDPOINT").ok(),
            azure_deployment_name: std::env::var("AZURE_OPENAI_DEPLOYMENT").ok(),
            azure_api_key: std::env::var("AZURE_OPENAI_API_KEY").ok(),
            azure_api_version: std::env::var("AZURE_OPENAI_API_VERSION").ok(),
            openai_api_key: std::env::var("OPENAI_API_KEY").ok(),
            openai_org_id: std::env::var("OPENAI_ORG_ID").ok(),
            output_dir,
            max_concurrent_jobs,
            default_model,
        };

        config.validate()?;
        Ok(config)
    }

    /// Validate that required fields are present for the chosen provider.
    pub(crate) fn validate(&self) -> Result<()> {
        match self.provider {
            Provider::Azure => {
                if self.azure_endpoint.is_none() {
                    return Err(ImagenError::ConfigError(
                        "AZURE_OPENAI_ENDPOINT is required for Azure provider".into(),
                    ));
                }
                if self.azure_api_key.is_none() {
                    return Err(ImagenError::ConfigError(
                        "AZURE_OPENAI_API_KEY is required for Azure provider".into(),
                    ));
                }
                if self.azure_deployment_name.is_none() {
                    return Err(ImagenError::ConfigError(
                        "AZURE_OPENAI_DEPLOYMENT is required for Azure provider".into(),
                    ));
                }
            }
            Provider::OpenAI => {
                if self.openai_api_key.is_none() {
                    return Err(ImagenError::ConfigError(
                        "OPENAI_API_KEY is required for OpenAI provider".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore] // This test mutates process environment variables without synchronization.
    // Cargo runs tests in parallel, so set_var/remove_var can race with other tests.
    // Use `cargo test -- --ignored` to run it in isolation.
    fn test_config_from_env_openai() {
        // Set minimal env vars for OpenAI
        std::env::set_var("IMAGEN_PROVIDER", "openai");
        std::env::set_var("OPENAI_API_KEY", "sk-test-key");
        std::env::set_var("IMAGEN_OUTPUT_DIR", "/tmp/imagen-test");

        let config = AppConfig::from_env().unwrap();
        assert_eq!(config.provider, Provider::OpenAI);
        assert_eq!(config.openai_api_key, Some("sk-test-key".into()));
        assert_eq!(config.output_dir, "/tmp/imagen-test");
        assert_eq!(config.max_concurrent_jobs, 4);
        assert_eq!(config.default_model, "gpt-image-2");

        // Clean up
        std::env::remove_var("IMAGEN_PROVIDER");
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("IMAGEN_OUTPUT_DIR");
    }

    #[test]
    fn test_validate_azure_missing_endpoint() {
        let config = AppConfig {
            provider: Provider::Azure,
            azure_endpoint: None,
            azure_deployment_name: Some("deploy".into()),
            azure_api_key: Some("key".into()),
            azure_api_version: None,
            openai_api_key: None,
            openai_org_id: None,
            output_dir: "./out".into(),
            max_concurrent_jobs: 4,
            default_model: "gpt-image-2".into(),
        };
        let result = config.validate();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("AZURE_OPENAI_ENDPOINT"));
    }

    #[test]
    fn test_validate_azure_missing_api_key() {
        let config = AppConfig {
            provider: Provider::Azure,
            azure_endpoint: Some("https://example.openai.azure.com".into()),
            azure_deployment_name: Some("deploy".into()),
            azure_api_key: None,
            azure_api_version: None,
            openai_api_key: None,
            openai_org_id: None,
            output_dir: "./out".into(),
            max_concurrent_jobs: 4,
            default_model: "gpt-image-2".into(),
        };
        let result = config.validate();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("AZURE_OPENAI_API_KEY"));
    }

    #[test]
    fn test_validate_azure_missing_deployment() {
        let config = AppConfig {
            provider: Provider::Azure,
            azure_endpoint: Some("https://example.openai.azure.com".into()),
            azure_deployment_name: None,
            azure_api_key: Some("key".into()),
            azure_api_version: None,
            openai_api_key: None,
            openai_org_id: None,
            output_dir: "./out".into(),
            max_concurrent_jobs: 4,
            default_model: "gpt-image-2".into(),
        };
        let result = config.validate();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("AZURE_OPENAI_DEPLOYMENT"));
    }

    #[test]
    fn test_validate_openai_missing_api_key() {
        let config = AppConfig {
            provider: Provider::OpenAI,
            azure_endpoint: None,
            azure_deployment_name: None,
            azure_api_key: None,
            azure_api_version: None,
            openai_api_key: None,
            openai_org_id: None,
            output_dir: "./out".into(),
            max_concurrent_jobs: 4,
            default_model: "gpt-image-2".into(),
        };
        let result = config.validate();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("OPENAI_API_KEY"));
    }

    #[test]
    fn test_validate_azure_all_present() {
        let config = AppConfig {
            provider: Provider::Azure,
            azure_endpoint: Some("https://example.openai.azure.com".into()),
            azure_deployment_name: Some("gpt-image-2".into()),
            azure_api_key: Some("azure-key-123".into()),
            azure_api_version: Some("2024-06-01".into()),
            openai_api_key: None,
            openai_org_id: None,
            output_dir: "./out".into(),
            max_concurrent_jobs: 4,
            default_model: "gpt-image-2".into(),
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_openai_all_present() {
        let config = AppConfig {
            provider: Provider::OpenAI,
            azure_endpoint: None,
            azure_deployment_name: None,
            azure_api_key: None,
            azure_api_version: None,
            openai_api_key: Some("sk-key-123".into()),
            openai_org_id: Some("org-123".into()),
            output_dir: "./out".into(),
            max_concurrent_jobs: 4,
            default_model: "gpt-image-2".into(),
        };
        assert!(config.validate().is_ok());
    }
}
