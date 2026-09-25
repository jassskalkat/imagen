use std::path::{Path, PathBuf};

use crate::error::{ImagenError, Result};
use serde::{Deserialize, Serialize};

const DEFAULT_OUTPUT_DIR: &str = "./imagen-output";
const DEFAULT_MODEL: &str = "gpt-image-2.5-sunburst";
const DEFAULT_MAX_CONCURRENT_JOBS: usize = 4;

/// Persistent configuration written by `imagen setup` and loaded at startup.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfigFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) provider: Option<Provider>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) azure_endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) azure_deployment_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) azure_api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) azure_api_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) openai_api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) openai_org_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) output_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) max_concurrent_jobs: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) default_model: Option<String>,
}

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
    /// Load configuration from environment variables only.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn from_env() -> Result<Self> {
        ConfigFile::from_env_vars()?.finalize()
    }

    /// Load configuration from the persisted config file, then override it with
    /// any environment variables that are set.
    pub fn load() -> Result<Self> {
        let persisted = ConfigFile::load_optional(&config_file_path())?.unwrap_or_default();
        let overlay = ConfigFile::from_env_vars()?;
        persisted.merge(overlay).finalize()
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

        // Create the output directory now so permission / path problems surface at startup
        // rather than silently failing on the first job.
        std::fs::create_dir_all(&self.output_dir).map_err(|e| {
            ImagenError::ConfigError(format!(
                "Cannot create output directory '{}': {e}",
                self.output_dir
            ))
        })?;

        Ok(())
    }
}

impl ConfigFile {
    /// Build a config snapshot from environment variables only.
    pub(crate) fn from_env_vars() -> Result<Self> {
        Ok(Self {
            provider: env_provider("IMAGEN_PROVIDER")?,
            azure_endpoint: env_string("AZURE_OPENAI_ENDPOINT"),
            azure_deployment_name: env_string("AZURE_OPENAI_DEPLOYMENT"),
            azure_api_key: env_string("AZURE_OPENAI_API_KEY"),
            azure_api_version: env_string("AZURE_OPENAI_API_VERSION"),
            openai_api_key: env_string("OPENAI_API_KEY"),
            openai_org_id: env_string("OPENAI_ORG_ID"),
            output_dir: env_string("IMAGEN_OUTPUT_DIR"),
            max_concurrent_jobs: env_usize("IMAGEN_MAX_CONCURRENT_JOBS")?,
            default_model: env_string("IMAGEN_DEFAULT_MODEL"),
        })
    }

    /// Load a config snapshot from disk if the file exists.
    pub(crate) fn load_optional(path: &Path) -> Result<Option<Self>> {
        if !path.exists() {
            return Ok(None);
        }

        let contents = std::fs::read_to_string(path).map_err(|e| {
            ImagenError::ConfigError(format!(
                "Failed to read config file '{}': {e}",
                path.display()
            ))
        })?;

        let file = serde_json::from_str(&contents).map_err(|e| {
            ImagenError::ConfigError(format!(
                "Failed to parse config file '{}': {e}",
                path.display()
            ))
        })?;

        Ok(Some(file))
    }

    /// Save the config snapshot to disk as pretty JSON.
    pub(crate) fn save_to_path(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                ImagenError::ConfigError(format!(
                    "Failed to create config directory '{}': {e}",
                    parent.display()
                ))
            })?;
            set_secure_dir_permissions(parent)?;
        }

        let mut options = std::fs::OpenOptions::new();
        options.create(true).truncate(true).write(true);

        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }

        let file = options.open(path).map_err(|e| {
            ImagenError::ConfigError(format!(
                "Failed to write config file '{}': {e}",
                path.display()
            ))
        })?;

        serde_json::to_writer_pretty(file, self).map_err(|e| {
            ImagenError::ConfigError(format!(
                "Failed to serialize config file '{}': {e}",
                path.display()
            ))
        })
    }

    /// Merge another snapshot over this one, preferring values from `other`.
    pub(crate) fn merge(mut self, other: Self) -> Self {
        self.provider = other.provider.or(self.provider);
        self.azure_endpoint = other.azure_endpoint.or(self.azure_endpoint);
        self.azure_deployment_name = other.azure_deployment_name.or(self.azure_deployment_name);
        self.azure_api_key = other.azure_api_key.or(self.azure_api_key);
        self.azure_api_version = other.azure_api_version.or(self.azure_api_version);
        self.openai_api_key = other.openai_api_key.or(self.openai_api_key);
        self.openai_org_id = other.openai_org_id.or(self.openai_org_id);
        self.output_dir = other.output_dir.or(self.output_dir);
        self.max_concurrent_jobs = other.max_concurrent_jobs.or(self.max_concurrent_jobs);
        self.default_model = other.default_model.or(self.default_model);
        self
    }

    /// Convert the persisted snapshot into the runtime config and validate it.
    pub(crate) fn finalize(self) -> Result<AppConfig> {
        let provider = self.provider.unwrap_or(Provider::OpenAI);
        let output_dir =
            sanitize(self.output_dir).unwrap_or_else(|| DEFAULT_OUTPUT_DIR.to_string());
        let max_concurrent_jobs = self
            .max_concurrent_jobs
            .unwrap_or(DEFAULT_MAX_CONCURRENT_JOBS);
        let default_model =
            sanitize(self.default_model).unwrap_or_else(|| DEFAULT_MODEL.to_string());

        let config = AppConfig {
            provider,
            azure_endpoint: sanitize(self.azure_endpoint),
            azure_deployment_name: sanitize(self.azure_deployment_name),
            azure_api_key: sanitize(self.azure_api_key),
            azure_api_version: sanitize(self.azure_api_version),
            openai_api_key: sanitize(self.openai_api_key),
            openai_org_id: sanitize(self.openai_org_id),
            output_dir,
            max_concurrent_jobs,
            default_model,
        };

        config.validate()?;
        Ok(config)
    }
}

pub(crate) fn config_file_path() -> PathBuf {
    if let Ok(path) = std::env::var("IMAGEN_CONFIG_FILE") {
        return PathBuf::from(path);
    }

    config_dir().join("config.json")
}

pub(crate) fn config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("IMAGEN_CONFIG_DIR") {
        return PathBuf::from(dir);
    }

    if cfg!(windows) {
        if let Some(base) = std::env::var_os("APPDATA") {
            return PathBuf::from(base).join("imagen");
        }
        if let Some(base) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(base).join("imagen");
        }
    } else {
        if let Some(base) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(base).join("imagen");
        }
    }

    home_dir()
        .map(|home| home.join(".config").join("imagen"))
        .unwrap_or_else(|| PathBuf::from(".imagen"))
}

pub(crate) fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("IMAGEN_DATA_DIR") {
        return PathBuf::from(dir);
    }

    if cfg!(windows) {
        if let Some(base) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(base).join("imagen");
        }
        if let Some(base) = std::env::var_os("APPDATA") {
            return PathBuf::from(base).join("imagen");
        }
    } else if let Some(base) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(base).join("imagen");
    }

    home_dir()
        .map(|home| home.join(".local").join("share").join("imagen"))
        .unwrap_or_else(|| PathBuf::from(".imagen"))
}

pub(crate) fn default_output_dir() -> String {
    data_dir().join("output").to_string_lossy().to_string()
}

fn config_string(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn env_string(name: &str) -> Option<String> {
    config_string(std::env::var(name).ok())
}

fn env_usize(name: &str) -> Result<Option<usize>> {
    match env_string(name) {
        Some(value) => value
            .parse()
            .map(Some)
            .map_err(|e| ImagenError::ConfigError(format!("Invalid {name}: {e}"))),
        None => Ok(None),
    }
}

fn env_provider(name: &str) -> Result<Option<Provider>> {
    match env_string(name) {
        Some(value) => match value.to_lowercase().as_str() {
            "azure" => Ok(Some(Provider::Azure)),
            "openai" => Ok(Some(Provider::OpenAI)),
            other => Err(ImagenError::ConfigError(format!(
                "Unknown provider: {other}. Expected 'azure' or 'openai'."
            ))),
        },
        None => Ok(None),
    }
}

fn sanitize(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

#[cfg(unix)]
fn set_secure_dir_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(|e| {
        ImagenError::ConfigError(format!(
            "Failed to secure config directory '{}': {e}",
            path.display()
        ))
    })
}

#[cfg(not(unix))]
fn set_secure_dir_permissions(_path: &Path) -> Result<()> {
    Ok(())
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
        assert_eq!(config.default_model, "gpt-image-2.5-sunburst");

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
            output_dir: "/tmp".into(),
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
            output_dir: "/tmp".into(),
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
            output_dir: "/tmp".into(),
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
            output_dir: "/tmp".into(),
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
            azure_api_version: Some("2025-04-01-preview".into()),
            openai_api_key: None,
            openai_org_id: None,
            output_dir: std::env::temp_dir()
                .join("imagen-config-test-azure")
                .to_string_lossy()
                .to_string(),
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
            output_dir: std::env::temp_dir()
                .join("imagen-config-test-openai")
                .to_string_lossy()
                .to_string(),
            max_concurrent_jobs: 4,
            default_model: "gpt-image-2".into(),
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_bad_output_dir_fails() {
        let config = AppConfig {
            provider: Provider::OpenAI,
            azure_endpoint: None,
            azure_deployment_name: None,
            azure_api_key: None,
            azure_api_version: None,
            openai_api_key: Some("sk-key-123".into()),
            openai_org_id: None,
            // /root is not writable by normal users
            output_dir: "/root/imagen-test-should-fail".into(),
            max_concurrent_jobs: 4,
            default_model: "gpt-image-2".into(),
        };
        // This test is meaningful only when not running as root.
        if std::env::var("USER").unwrap_or_default() != "root" {
            assert!(config.validate().is_err());
        }
    }

    #[test]
    fn test_config_file_merge_prefers_overlay() {
        let base = ConfigFile {
            provider: Some(Provider::Azure),
            azure_endpoint: Some("https://example.openai.azure.com".into()),
            azure_deployment_name: Some("deploy".into()),
            azure_api_key: Some("azure-key".into()),
            azure_api_version: Some("2025-04-01-preview".into()),
            openai_api_key: None,
            openai_org_id: None,
            output_dir: Some("/tmp/base-output".into()),
            max_concurrent_jobs: Some(2),
            default_model: Some("gpt-image-2".into()),
        };
        let overlay = ConfigFile {
            provider: Some(Provider::OpenAI),
            azure_endpoint: None,
            azure_deployment_name: None,
            azure_api_key: None,
            azure_api_version: None,
            openai_api_key: Some("sk-overlay".into()),
            openai_org_id: Some("org-overlay".into()),
            output_dir: Some("/tmp/overlay-output".into()),
            max_concurrent_jobs: Some(8),
            default_model: Some("custom-model".into()),
        };

        let merged = base.merge(overlay);
        assert_eq!(merged.provider, Some(Provider::OpenAI));
        assert_eq!(merged.openai_api_key, Some("sk-overlay".into()));
        assert_eq!(merged.openai_org_id, Some("org-overlay".into()));
        assert_eq!(merged.output_dir, Some("/tmp/overlay-output".into()));
        assert_eq!(merged.max_concurrent_jobs, Some(8));
        assert_eq!(merged.default_model, Some("custom-model".into()));
        assert_eq!(
            merged.azure_endpoint,
            Some("https://example.openai.azure.com".into())
        );
    }

    #[test]
    fn test_config_file_finalize_uses_defaults() {
        let config = ConfigFile {
            provider: Some(Provider::OpenAI),
            azure_endpoint: None,
            azure_deployment_name: None,
            azure_api_key: None,
            azure_api_version: None,
            openai_api_key: Some("sk-test".into()),
            openai_org_id: None,
            output_dir: Some(
                std::env::temp_dir()
                    .join("imagen-config-finalize")
                    .to_string_lossy()
                    .to_string(),
            ),
            max_concurrent_jobs: None,
            default_model: None,
        };

        let runtime = config.finalize().unwrap();
        assert_eq!(runtime.provider, Provider::OpenAI);
        assert_eq!(runtime.max_concurrent_jobs, DEFAULT_MAX_CONCURRENT_JOBS);
        assert_eq!(runtime.default_model, DEFAULT_MODEL);
    }

    #[test]
    fn test_config_file_round_trip() {
        let dir = std::env::temp_dir().join("imagen-config-roundtrip");
        let path = dir.join("config.json");
        let config = ConfigFile {
            provider: Some(Provider::OpenAI),
            azure_endpoint: None,
            azure_deployment_name: None,
            azure_api_key: None,
            azure_api_version: None,
            openai_api_key: Some("sk-test".into()),
            openai_org_id: Some("org-test".into()),
            output_dir: Some("/tmp/imagen-output".into()),
            max_concurrent_jobs: Some(3),
            default_model: Some("gpt-image-2".into()),
        };

        config.save_to_path(&path).unwrap();
        let loaded = ConfigFile::load_optional(&path).unwrap().unwrap();
        assert_eq!(loaded.provider, config.provider);
        assert_eq!(loaded.openai_api_key, config.openai_api_key);
        assert_eq!(loaded.openai_org_id, config.openai_org_id);
        assert_eq!(loaded.output_dir, config.output_dir);
        assert_eq!(loaded.max_concurrent_jobs, config.max_concurrent_jobs);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
