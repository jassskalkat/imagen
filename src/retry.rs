use std::future::Future;
use std::time::Duration;

use rand::Rng;
use tracing::warn;

use crate::error::{ImagenError, Result};

/// Maximum number of retry attempts.
const MAX_RETRIES: u32 = 3;
/// Base delay between retries (1 second).
const BASE_DELAY: Duration = Duration::from_secs(1);
/// Multiplier for exponential backoff.
const BACKOFF_FACTOR: u32 = 2;

/// Determine whether an error is transient and should be retried.
fn is_retryable(err: &ImagenError) -> bool {
    err.is_transient()
}

/// Extract a service-provided retry delay hint, if present.
fn retry_after_delay(err: &ImagenError) -> Option<Duration> {
    let message = match err {
        ImagenError::RateLimit(message) => message.as_str(),
        ImagenError::ProviderError {
            message,
            status_code: Some(429),
        } => message.as_str(),
        _ => return None,
    };

    extract_retry_after_seconds(message).map(Duration::from_secs)
}

/// Parse "retry after N seconds" style messages emitted by Azure/OpenAI.
fn extract_retry_after_seconds(message: &str) -> Option<u64> {
    let lower = message.to_ascii_lowercase();
    let start = lower.find("retry after")?;
    let mut digits = String::new();
    let mut seen_digit = false;

    for ch in message[start + "retry after".len()..].chars() {
        if ch.is_ascii_digit() {
            seen_digit = true;
            digits.push(ch);
        } else if seen_digit {
            break;
        }
    }

    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

/// Execute an async operation with exponential backoff retry.
///
/// Retries up to `MAX_RETRIES` times on transient errors (rate limits and
/// 5xx provider errors). Uses exponential backoff with a small random jitter
/// to avoid thundering herd.
pub async fn with_retry<F, Fut, T>(operation: F) -> Result<T>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut attempt = 0;

    loop {
        match operation().await {
            Ok(value) => return Ok(value),
            Err(err) => {
                if !is_retryable(&err) || attempt >= MAX_RETRIES {
                    return Err(err);
                }

                let base_ms = BASE_DELAY.as_millis() as u64 * (BACKOFF_FACTOR.pow(attempt) as u64);
                let jitter_ms = rand::thread_rng().gen_range(0..=100);
                let mut delay = Duration::from_millis(base_ms + jitter_ms);
                let retry_after_hint = retry_after_delay(&err);
                if let Some(hint) = retry_after_hint {
                    if hint > delay {
                        delay = hint;
                    }
                }

                warn!(
                    attempt = attempt + 1,
                    max_retries = MAX_RETRIES,
                    delay_ms = delay.as_millis() as u64,
                    retry_after_ms = retry_after_hint.map(|d| d.as_millis() as u64),
                    error = %err,
                    "Retrying after transient error"
                );

                tokio::time::sleep(delay).await;
                attempt += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_succeeds_on_first_try() {
        let call_count = Arc::new(AtomicU32::new(0));
        let count = call_count.clone();

        let result = with_retry(|| {
            let count = count.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                Ok::<_, ImagenError>(42)
            }
        })
        .await;

        assert_eq!(result.unwrap(), 42);
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_retries_on_rate_limit() {
        let call_count = Arc::new(AtomicU32::new(0));
        let count = call_count.clone();

        let result = with_retry(|| {
            let count = count.clone();
            async move {
                let n = count.fetch_add(1, Ordering::SeqCst);
                if n < 2 {
                    Err(ImagenError::RateLimit("rate limited".into()))
                } else {
                    Ok::<_, ImagenError>("success")
                }
            }
        })
        .await;

        assert_eq!(result.unwrap(), "success");
        assert_eq!(call_count.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_stops_after_max_retries() {
        let call_count = Arc::new(AtomicU32::new(0));
        let count = call_count.clone();

        let result = with_retry(|| {
            let count = count.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                Err::<(), _>(ImagenError::RateLimit("always rate limited".into()))
            }
        })
        .await;

        assert!(result.is_err());
        // 1 initial attempt + 3 retries = 4 total calls
        assert_eq!(call_count.load(Ordering::SeqCst), 4);
    }

    #[tokio::test]
    async fn test_non_retryable_error_not_retried() {
        let call_count = Arc::new(AtomicU32::new(0));
        let count = call_count.clone();

        let result = with_retry(|| {
            let count = count.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                Err::<(), _>(ImagenError::InvalidInput("bad input".into()))
            }
        })
        .await;

        assert!(result.is_err());
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_retries_on_5xx_provider_error() {
        let call_count = Arc::new(AtomicU32::new(0));
        let count = call_count.clone();

        let result = with_retry(|| {
            let count = count.clone();
            async move {
                let n = count.fetch_add(1, Ordering::SeqCst);
                if n < 1 {
                    Err(ImagenError::ProviderError {
                        message: "Azure API error: server error".into(),
                        status_code: Some(503),
                    })
                } else {
                    Ok::<_, ImagenError>("recovered")
                }
            }
        })
        .await;

        assert_eq!(result.unwrap(), "recovered");
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_does_not_retry_auth_error() {
        let call_count = Arc::new(AtomicU32::new(0));
        let count = call_count.clone();

        let result = with_retry(|| {
            let count = count.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                Err::<(), _>(ImagenError::ProviderAuth("unauthorized".into()))
            }
        })
        .await;

        assert!(result.is_err());
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_extract_retry_after_seconds() {
        assert_eq!(
            extract_retry_after_seconds("Please retry after 18 seconds."),
            Some(18)
        );
        assert_eq!(
            extract_retry_after_seconds("Retry after 2 seconds and try again."),
            Some(2)
        );
        assert_eq!(extract_retry_after_seconds("No retry hint here"), None);
    }
}
