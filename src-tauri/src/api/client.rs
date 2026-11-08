use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use reqwest::{Client, Response, StatusCode};
use serde::de::DeserializeOwned;

use crate::api::USER_AGENT;
use crate::error::{AppError, AppResult};

static CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
static DOWNLOAD_CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
static REQUEST_GATE: OnceLock<tokio::sync::Mutex<Instant>> = OnceLock::new();

/// Minimum gap between GameBanana request starts. The lock is held for the
/// whole JSON request (including reading the body), so one slow call cannot
/// cause a burst of queued calls to hit the API together.
const REQUEST_GAP: Duration = Duration::from_millis(300);

/// Initial request plus three retries.
const MAX_ATTEMPTS: u32 = 4;

/// Do not sleep for longer than this inside the app. If GameBanana asks for a
/// longer pause, return a clear error instead of freezing the UI for minutes.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);

const BASE_BACKOFF: Duration = Duration::from_millis(750);
const JITTER_MS: u64 = 400;

/// Shared HTTP client: one connection pool, a stable User-Agent and timeouts so
/// a hanging request can never wedge the UI.
///
/// A construction failure (TLS backend, proxy configuration) is reported as a
/// network error rather than panicking: the release profile uses
/// `panic = "abort"`, so a panic here would close the whole application.
pub fn client() -> AppResult<&'static Client> {
    match CLIENT.get_or_init(|| {
        Client::builder()
            .user_agent(USER_AGENT.as_str())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|error| format!("Could not start the HTTP client: {error}"))
    }) {
        Ok(client) => Ok(client),
        Err(message) => Err(AppError::Network(message.clone())),
    }
}

/// Client for downloading archive payloads.
///
/// The browse client has a 30 s *total* timeout, which is right for small JSON
/// calls but aborts a healthy large download: a 90 MB mod at ~2 MB/s needs
/// about a minute, and reqwest surfaced the abort as an opaque
/// `error decoding response body`. This client has no total timeout, a read
/// timeout so a stalled connection still fails, and gzip disabled so the bytes
/// on disk are exactly the archive that was served.
pub fn download_client() -> AppResult<&'static Client> {
    match DOWNLOAD_CLIENT.get_or_init(|| {
        Client::builder()
            .user_agent(USER_AGENT.as_str())
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(60))
            .gzip(false)
            .build()
            .map_err(|error| format!("Could not start the download client: {error}"))
    }) {
        Ok(client) => Ok(client),
        Err(message) => Err(AppError::Network(message.clone())),
    }
}

async fn acquire_request_slot() -> tokio::sync::MutexGuard<'static, Instant> {
    let gate = REQUEST_GATE.get_or_init(|| {
        tokio::sync::Mutex::new(
            Instant::now()
                .checked_sub(REQUEST_GAP)
                .unwrap_or_else(Instant::now),
        )
    });

    let mut last = gate.lock().await;
    let elapsed = last.elapsed();
    if elapsed < REQUEST_GAP {
        tokio::time::sleep(REQUEST_GAP - elapsed).await;
    }
    *last = Instant::now();
    last
}

/// Space out a request that does not need the exclusive JSON gate (downloads
/// stream for much longer than a metadata call).
async fn wait_for_spacing() {
    drop(acquire_request_slot().await);
}

fn retry_after(response: &Response) -> Option<Duration> {
    response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
}

fn backoff_delay(attempt: u32) -> Duration {
    // 750 ms, 1.5 s, 3 s, ... plus a little jitter so a fleet of clients that
    // hit the same limit does not retry in lockstep.
    let multiplier = 1u64 << attempt.min(4);
    let base = BASE_BACKOFF.as_millis() as u64 * multiplier;
    let jitter = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.subsec_millis() as u64 % JITTER_MS)
        .unwrap_or(0);

    Duration::from_millis(base.saturating_add(jitter))
}

fn retry_delay(attempt: u32, retry_after: Option<Duration>) -> Option<Duration> {
    match retry_after {
        Some(delay) if delay > MAX_RETRY_AFTER => None,
        Some(delay) if delay.is_zero() => Some(Duration::from_secs(1)),
        Some(delay) => Some(delay),
        None => Some(backoff_delay(attempt)),
    }
}

fn rate_limit_message(retry_after: Option<Duration>) -> String {
    match retry_after {
        Some(delay) => format!(
            "GameBanana is rate-limiting ZZMM. It asked us to wait {} seconds; try again after that.",
            delay.as_secs().max(1)
        ),
        None => "GameBanana is rate-limiting ZZMM. Wait a minute and try again.".to_string(),
    }
}

fn status_error(url: &str, status: StatusCode, retry_after: Option<Duration>) -> AppError {
    match status {
        StatusCode::NOT_FOUND => {
            AppError::GameBanana(format!("GameBanana has no entry at {url} (404)."))
        }
        StatusCode::TOO_MANY_REQUESTS => AppError::GameBanana(rate_limit_message(retry_after)),
        StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => {
            AppError::GameBanana("GameBanana refused the request. Try again later.".to_string())
        }
        other => AppError::GameBanana(format!(
            "GameBanana answered with HTTP {} for {url}.",
            other.as_u16()
        )),
    }
}

pub async fn get_json<T: DeserializeOwned>(url: &str) -> AppResult<T> {
    let mut attempt = 0;

    loop {
        let slot = acquire_request_slot().await;
        let response = client()?
            .get(url)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|error| AppError::Network(error.to_string()))?;

        let status = response.status();
        let retry_after = retry_after(&response);

        if status == StatusCode::TOO_MANY_REQUESTS {
            if attempt + 1 < MAX_ATTEMPTS {
                if let Some(delay) = retry_delay(attempt, retry_after) {
                    drop(response);
                    drop(slot);

                    let retry = attempt + 1;
                    log::warn!(
                        "GameBanana rate-limited {url}; retrying in {} ms (retry {retry}/{})",
                        delay.as_millis(),
                        MAX_ATTEMPTS - 1
                    );
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                    continue;
                }
            }

            return Err(status_error(url, status, retry_after));
        }

        if !status.is_success() {
            return Err(status_error(url, status, retry_after));
        }

        return response.json::<T>().await.map_err(|error| {
            AppError::GameBanana(format!("Unexpected GameBanana response: {error}"))
        });
    }
}

/// Send a download request with the same 429 handling as metadata calls. The
/// response body is returned unread so the caller can stream it with progress.
pub async fn send_download_request(url: &str) -> AppResult<Response> {
    let mut attempt = 0;

    loop {
        wait_for_spacing().await;
        let response = download_client()?
            .get(url)
            .send()
            .await
            .map_err(|error| AppError::Download(error.to_string()))?;

        let status = response.status();
        let retry_after = retry_after(&response);

        if status == StatusCode::TOO_MANY_REQUESTS {
            if attempt + 1 < MAX_ATTEMPTS {
                if let Some(delay) = retry_delay(attempt, retry_after) {
                    drop(response);

                    let retry = attempt + 1;
                    log::warn!(
                        "GameBanana rate-limited download {url}; retrying in {} ms (retry {retry}/{})",
                        delay.as_millis(),
                        MAX_ATTEMPTS - 1
                    );
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                    continue;
                }
            }

            return Err(AppError::Download(rate_limit_message(retry_after)));
        }

        return Ok(response);
    }
}

/// Percent-encode a query value. Only RFC 3986 unreserved characters survive.
pub fn encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());

    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(*byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }

    encoded
}

#[cfg(test)]
mod tests {
    use super::{backoff_delay, encode, retry_delay, MAX_RETRY_AFTER};
    use std::time::Duration;

    #[test]
    fn encodes_query_values() {
        assert_eq!(encode("ellen joe"), "ellen%20joe");
        assert_eq!(encode("a&b=c"), "a%26b%3Dc");
        assert_eq!(encode("Yixuan"), "Yixuan");
    }

    #[test]
    fn honours_retry_after_with_a_sane_floor() {
        assert_eq!(
            retry_delay(0, Some(Duration::from_secs(3))),
            Some(Duration::from_secs(3))
        );
        assert_eq!(
            retry_delay(0, Some(Duration::ZERO)),
            Some(Duration::from_secs(1))
        );
    }

    #[test]
    fn refuses_retry_after_values_that_would_freeze_the_ui() {
        assert_eq!(retry_delay(0, Some(MAX_RETRY_AFTER)), Some(MAX_RETRY_AFTER));
        assert_eq!(
            retry_delay(0, Some(MAX_RETRY_AFTER + Duration::from_secs(1))),
            None
        );
    }

    #[test]
    fn exponential_backoff_grows_between_attempts() {
        assert!(backoff_delay(0) >= Duration::from_millis(750));
        assert!(backoff_delay(0) < Duration::from_millis(1_200));
        assert!(backoff_delay(1) >= Duration::from_millis(1_500));
        assert!(backoff_delay(3) >= Duration::from_millis(6_000));
    }
}
