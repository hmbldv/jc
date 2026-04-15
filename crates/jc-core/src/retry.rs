//! HTTP retry helpers.
//!
//! Atlassian uses dynamic cost-based rate limiting that returns HTTP 429
//! with a `Retry-After` header when a client exceeds its budget. This
//! module wraps individual requests in a bounded retry loop so callers
//! don't have to manually reshoot the same call after a rate limit.
//!
//! Retry policy is per-verb, selected at the call site:
//!
//! - [`RetryPolicy::Read`] — retry on 429, 502, 503, 504. Used for GET
//!   and download endpoints since they're idempotent and safe to replay.
//! - [`RetryPolicy::IdempotencySafe`] — retry only on 429. Atlassian
//!   documents 429 as "request never processed, rate-limited before
//!   dispatch," so replaying is safe. Other 5xx responses might indicate
//!   partial processing; we don't retry those on mutations to avoid
//!   double-committing a comment/transition/etc.
//! - [`RetryPolicy::None`] — single attempt. Used for multipart uploads
//!   (the `reqwest::multipart::Form` is move-consumed and can't be
//!   rebuilt without re-reading the source file).
//!
//! The retry count is bounded at [`MAX_ATTEMPTS`]. When the server's
//! `Retry-After` value exceeds [`MAX_RETRY_AFTER`] we bail out early and
//! let the 429 propagate — a CLI shouldn't block for minutes on end.

use std::time::Duration;

use reqwest::{Response, StatusCode};
use tracing::warn;

use crate::error::{ApiError, Result};

/// Hard cap on retry attempts. With the current backoff schedule, four
/// attempts (one initial + three retries) adds up to ~3.5s of waiting.
pub const MAX_ATTEMPTS: u32 = 4;

/// If the server's `Retry-After` exceeds this, we give up and return the
/// 429 instead of blocking the CLI for minutes. Users can re-run.
pub const MAX_RETRY_AFTER: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryPolicy {
    /// Single attempt; no retry. For multipart uploads and other
    /// non-replayable requests.
    None,
    /// Retry only on 429. Safe for mutations because 429 means the
    /// server never processed the request.
    IdempotencySafe,
    /// Retry on 429 and read-safe 5xx (502, 503, 504). For GET + download.
    Read,
}

impl RetryPolicy {
    /// Should a given HTTP status trigger a retry under this policy?
    pub fn should_retry(self, status: StatusCode) -> bool {
        match self {
            Self::None => false,
            Self::IdempotencySafe => status == StatusCode::TOO_MANY_REQUESTS,
            Self::Read => matches!(
                status,
                StatusCode::TOO_MANY_REQUESTS
                    | StatusCode::BAD_GATEWAY
                    | StatusCode::SERVICE_UNAVAILABLE
                    | StatusCode::GATEWAY_TIMEOUT
            ),
        }
    }
}

/// Send a request, retrying transient failures according to `policy`.
///
/// `build` is called once per attempt because reqwest's `RequestBuilder`
/// is move-consumed on `send()`. Each attempt gets a fresh builder from
/// the closure.
///
/// Returns the final `Response` regardless of status — the caller's
/// normal parse_response / parse_empty path will surface the error if
/// the retries exhausted.
pub async fn send_with_retry<F>(build: F, policy: RetryPolicy) -> Result<Response>
where
    F: Fn() -> reqwest::RequestBuilder,
{
    let mut attempt: u32 = 1;
    loop {
        let resp = build().send().await.map_err(ApiError::transport)?;
        let status = resp.status();

        if status.is_success() {
            return Ok(resp);
        }
        if !policy.should_retry(status) || attempt >= MAX_ATTEMPTS {
            return Ok(resp);
        }

        let wait = retry_after(&resp).unwrap_or_else(|| backoff(attempt));
        if wait > MAX_RETRY_AFTER {
            // Server wants us to wait longer than our circuit breaker
            // allows. Surface the 429 so the user can retry manually.
            return Ok(resp);
        }

        warn!(
            attempt,
            status = %status,
            wait_ms = wait.as_millis() as u64,
            "retryable http error, sleeping before retry"
        );
        tokio::time::sleep(wait).await;
        attempt += 1;
    }
}

/// Parse the `Retry-After` header as a number of seconds. HTTP-date
/// form is accepted by the spec but Atlassian always sends an integer,
/// so we only parse that form.
fn retry_after(resp: &Response) -> Option<Duration> {
    resp.headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

/// Exponential backoff schedule: 500ms, 1s, 2s, 4s, ... (capped).
fn backoff(attempt: u32) -> Duration {
    let shift = (attempt - 1).min(6);
    let ms = 500u64 * (1u64 << shift);
    Duration::from_millis(ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows() {
        assert_eq!(backoff(1), Duration::from_millis(500));
        assert_eq!(backoff(2), Duration::from_millis(1000));
        assert_eq!(backoff(3), Duration::from_millis(2000));
        assert_eq!(backoff(4), Duration::from_millis(4000));
    }

    #[test]
    fn backoff_caps_at_shift_6() {
        // After attempt 7 the shift clamps so we don't overflow.
        let big = backoff(100);
        assert!(big <= Duration::from_secs(64));
    }

    #[test]
    fn policy_read_retries_429() {
        assert!(RetryPolicy::Read.should_retry(StatusCode::TOO_MANY_REQUESTS));
    }

    #[test]
    fn policy_read_retries_5xx() {
        assert!(RetryPolicy::Read.should_retry(StatusCode::BAD_GATEWAY));
        assert!(RetryPolicy::Read.should_retry(StatusCode::SERVICE_UNAVAILABLE));
        assert!(RetryPolicy::Read.should_retry(StatusCode::GATEWAY_TIMEOUT));
    }

    #[test]
    fn policy_read_does_not_retry_500() {
        // 500 is "the server hit a bug"; retrying usually doesn't help
        // and adds noise. Let the user rerun manually.
        assert!(!RetryPolicy::Read.should_retry(StatusCode::INTERNAL_SERVER_ERROR));
    }

    #[test]
    fn policy_idempotency_retries_only_429() {
        assert!(RetryPolicy::IdempotencySafe.should_retry(StatusCode::TOO_MANY_REQUESTS));
        assert!(!RetryPolicy::IdempotencySafe.should_retry(StatusCode::SERVICE_UNAVAILABLE));
        assert!(!RetryPolicy::IdempotencySafe.should_retry(StatusCode::INTERNAL_SERVER_ERROR));
    }

    #[test]
    fn policy_none_never_retries() {
        assert!(!RetryPolicy::None.should_retry(StatusCode::TOO_MANY_REQUESTS));
        assert!(!RetryPolicy::None.should_retry(StatusCode::SERVICE_UNAVAILABLE));
    }

    #[test]
    fn policy_never_retries_4xx_other_than_429() {
        for policy in [RetryPolicy::Read, RetryPolicy::IdempotencySafe] {
            assert!(!policy.should_retry(StatusCode::BAD_REQUEST));
            assert!(!policy.should_retry(StatusCode::UNAUTHORIZED));
            assert!(!policy.should_retry(StatusCode::FORBIDDEN));
            assert!(!policy.should_retry(StatusCode::NOT_FOUND));
            assert!(!policy.should_retry(StatusCode::CONFLICT));
        }
    }
}
