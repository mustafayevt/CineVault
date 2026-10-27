//! 429 handling: how long to wait and how often to retry.

use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, SystemTime};

/// Retry limits for rate-limited (HTTP 429) responses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Retries after the first attempt, so a request is sent at most `max_retries + 1` times.
    pub max_retries: u32,
    /// The longest a single wait may be, whatever `Retry-After` asks for.
    pub max_wait: Duration,
    /// The wait when the response has no usable `Retry-After`: doubled for each retry.
    pub base_backoff: Duration,
}

impl Default for RetryPolicy {
    /// Up to 3 retries, each waiting at most 30 seconds.
    fn default() -> Self {
        Self {
            max_retries: 3,
            max_wait: Duration::from_secs(30),
            base_backoff: Duration::from_secs(1),
        }
    }
}

impl RetryPolicy {
    /// The wait before retry number `retry` (0-based) given the server's `Retry-After`, capped.
    #[must_use]
    pub fn wait_for(&self, retry: u32, retry_after: Option<Duration>) -> Duration {
        let wanted = retry_after
            .unwrap_or_else(|| self.base_backoff.saturating_mul(2u32.saturating_pow(retry)));
        wanted.min(self.max_wait)
    }
}

/// Parses a `Retry-After` header: whole seconds, or an HTTP date (relative to `now`).
#[must_use]
pub fn parse_retry_after(value: &str, now: SystemTime) -> Option<Duration> {
    let value = value.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let at = httpdate::parse_http_date(value).ok()?;
    Some(at.duration_since(now).unwrap_or(Duration::ZERO))
}

/// How the client waits. Production code sleeps; tests substitute a recorder so a 30-second cap
/// can be checked without waiting 30 seconds.
pub trait Pause: Send + Sync {
    fn pause(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}

/// Sleeps with the tokio timer.
#[derive(Debug, Default, Clone, Copy)]
pub struct TokioPause;

impl Pause for TokioPause {
    fn pause(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(tokio::time::sleep(duration))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_seconds_and_dates() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        assert_eq!(parse_retry_after("2", now), Some(Duration::from_secs(2)));
        assert_eq!(parse_retry_after(" 0 ", now), Some(Duration::ZERO));
        let later = httpdate::fmt_http_date(now + Duration::from_secs(10));
        assert_eq!(
            parse_retry_after(&later, now),
            Some(Duration::from_secs(10))
        );
        let past = httpdate::fmt_http_date(now - Duration::from_secs(10));
        assert_eq!(parse_retry_after(&past, now), Some(Duration::ZERO));
        assert_eq!(parse_retry_after("soon", now), None);
        assert_eq!(parse_retry_after("-3", now), None);
    }

    #[test]
    fn waits_are_capped_and_back_off_without_a_header() {
        let policy = RetryPolicy::default();
        assert_eq!(
            policy.wait_for(0, Some(Duration::from_secs(2))),
            Duration::from_secs(2)
        );
        assert_eq!(
            policy.wait_for(0, Some(Duration::from_secs(600))),
            Duration::from_secs(30)
        );
        assert_eq!(policy.wait_for(0, None), Duration::from_secs(1));
        assert_eq!(policy.wait_for(2, None), Duration::from_secs(4));
        assert_eq!(policy.wait_for(30, None), Duration::from_secs(30));
    }
}
