use thiserror::Error;

/// Central error type for the imagen MCP server.
#[derive(Debug, Error)]
pub enum ImagenError {
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Provider authentication failed: {0}")]
    ProviderAuth(String),

    #[error("Rate limit exceeded: {0}")]
    RateLimit(String),

    #[error("Provider error: {message}")]
    ProviderError {
        message: String,
        /// HTTP status code from the provider, if available.
        status_code: Option<u16>,
    },

    #[error("Job not found: {0}")]
    JobNotFound(String),

    #[error("Session expired: {0}")]
    SessionExpired(String),

    #[error("File error: {0}")]
    FileError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<std::io::Error> for ImagenError {
    fn from(err: std::io::Error) -> Self {
        ImagenError::FileError(err.to_string())
    }
}

impl From<serde_json::Error> for ImagenError {
    fn from(err: serde_json::Error) -> Self {
        ImagenError::Internal(format!("Serialization error: {err}"))
    }
}

impl From<reqwest::Error> for ImagenError {
    fn from(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            ImagenError::ProviderError {
                message: format!("Request timed out: {err}"),
                status_code: None,
            }
        } else if err.is_status() {
            let status = err.status();
            match status.map(|s| s.as_u16()) {
                Some(401) | Some(403) => {
                    ImagenError::ProviderAuth(format!("Authentication failed: {err}"))
                }
                Some(429) => ImagenError::RateLimit(format!("Rate limited: {err}")),
                Some(code) => ImagenError::ProviderError {
                    message: err.to_string(),
                    status_code: Some(code),
                },
                None => ImagenError::ProviderError {
                    message: err.to_string(),
                    status_code: None,
                },
            }
        } else {
            ImagenError::ProviderError {
                message: err.to_string(),
                status_code: None,
            }
        }
    }
}

pub type Result<T> = std::result::Result<T, ImagenError>;

impl ImagenError {
    /// Returns true if this error is transient and the operation should be retried.
    pub fn is_transient(&self) -> bool {
        match self {
            ImagenError::RateLimit(_) => true,
            ImagenError::ProviderError {
                status_code,
                message,
            } => {
                // Retry on 5xx status codes
                if let Some(code) = status_code {
                    return *code >= 500 && *code < 600;
                }
                // Fallback: retry on timeout errors (no status code available)
                message.contains("timed out") || message.contains("timeout")
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = ImagenError::InvalidInput("bad prompt".into());
        assert_eq!(err.to_string(), "Invalid input: bad prompt");
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let err: ImagenError = io_err.into();
        assert!(matches!(err, ImagenError::FileError(_)));
        assert!(err.to_string().contains("file missing"));
    }

    #[test]
    fn test_serde_json_error_conversion() {
        // Create a serde_json error by trying to parse invalid JSON
        let json_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        let err: ImagenError = json_err.into();
        assert!(matches!(err, ImagenError::Internal(_)));
        assert!(err.to_string().contains("Serialization error"));
    }

    #[test]
    fn test_all_error_variants_display() {
        let cases = vec![
            (
                ImagenError::InvalidInput("test".into()),
                "Invalid input: test",
            ),
            (
                ImagenError::ProviderAuth("unauthorized".into()),
                "Provider authentication failed: unauthorized",
            ),
            (
                ImagenError::RateLimit("slow down".into()),
                "Rate limit exceeded: slow down",
            ),
            (
                ImagenError::ProviderError {
                    message: "500".into(),
                    status_code: Some(500),
                },
                "Provider error: 500",
            ),
            (ImagenError::JobNotFound("abc".into()), "Job not found: abc"),
            (
                ImagenError::SessionExpired("sess-1".into()),
                "Session expired: sess-1",
            ),
            (
                ImagenError::FileError("no file".into()),
                "File error: no file",
            ),
            (
                ImagenError::ConfigError("bad config".into()),
                "Configuration error: bad config",
            ),
            (ImagenError::Internal("oops".into()), "Internal error: oops"),
        ];

        for (err, expected) in cases {
            assert_eq!(err.to_string(), expected);
        }
    }

    #[test]
    fn test_io_error_not_found_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "path not found");
        let err: ImagenError = io_err.into();
        assert!(matches!(err, ImagenError::FileError(_)));
    }

    #[test]
    fn test_io_error_permission_denied_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let err: ImagenError = io_err.into();
        assert!(matches!(err, ImagenError::FileError(_)));
        assert!(err.to_string().contains("access denied"));
    }

    #[test]
    fn test_is_transient_rate_limit() {
        let err = ImagenError::RateLimit("slow down".into());
        assert!(err.is_transient());
    }

    #[test]
    fn test_is_transient_5xx_status() {
        let err = ImagenError::ProviderError {
            message: "server error".into(),
            status_code: Some(500),
        };
        assert!(err.is_transient());

        let err = ImagenError::ProviderError {
            message: "bad gateway".into(),
            status_code: Some(502),
        };
        assert!(err.is_transient());

        let err = ImagenError::ProviderError {
            message: "service unavailable".into(),
            status_code: Some(503),
        };
        assert!(err.is_transient());
    }

    #[test]
    fn test_is_transient_timeout_no_status() {
        let err = ImagenError::ProviderError {
            message: "Request timed out: connection error".into(),
            status_code: None,
        };
        assert!(err.is_transient());
    }

    #[test]
    fn test_is_not_transient_4xx() {
        let err = ImagenError::ProviderError {
            message: "bad request".into(),
            status_code: Some(400),
        };
        assert!(!err.is_transient());

        let err = ImagenError::ProviderError {
            message: "not found".into(),
            status_code: Some(404),
        };
        assert!(!err.is_transient());
    }

    #[test]
    fn test_is_not_transient_auth() {
        let err = ImagenError::ProviderAuth("unauthorized".into());
        assert!(!err.is_transient());
    }

    #[test]
    fn test_is_not_transient_invalid_input() {
        let err = ImagenError::InvalidInput("bad".into());
        assert!(!err.is_transient());
    }
}
