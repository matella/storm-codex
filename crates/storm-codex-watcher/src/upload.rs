//! Upload client for `POST {base}/api/upload` (protocol of `client-rs`).
//!
//! The request body is the raw replay (`Content-Type: application/octet-stream`); the file
//! name travels percent-encoded in `X-Filename`. Answers map to [`UploadResult`]:
//! 2xx → `Uploaded`, 409 → `Duplicate`, any other 4xx → `Rejected(body)` (never retried),
//! network error / 5xx → retried with a `2^n` backoff, `Retryable` once
//! [`MAX_ATTEMPTS`] are spent. A `Retryable` file must not be recorded in the ledger.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tokio::time::sleep;

/// Attempts per file before giving up with [`UploadResult::Retryable`].
pub const MAX_ATTEMPTS: u32 = 5;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
/// Rejection reasons are shown in the UI and stored in the ledger: keep them short.
const MAX_REASON_CHARS: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadResult {
    Uploaded,
    Duplicate,
    /// Refused by the server (4xx other than 409), with the response body.
    Rejected(String),
    /// Network error or 5xx still failing after every attempt (or the file was unreadable).
    Retryable(String),
}

#[derive(Debug, Clone)]
pub struct Uploader {
    url: String,
    agent: Arc<ureq::Agent>,
    backoff_unit: Duration,
}

impl Uploader {
    /// `base_url` is the server root (e.g. `http://127.0.0.1:8080`); a trailing `/` is ignored.
    pub fn new(base_url: String) -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(REQUEST_TIMEOUT))
            .http_status_as_error(false)
            .build();
        Uploader {
            url: format!("{}/api/upload", base_url.trim_end_matches('/')),
            agent: Arc::new(config.into()),
            backoff_unit: Duration::from_secs(1),
        }
    }

    /// Scales the retry backoff (`unit * 2^n`, default unit 1 s). Meant for tests.
    pub fn with_backoff_unit(mut self, unit: Duration) -> Self {
        self.backoff_unit = unit;
        self
    }

    /// Reads and uploads one replay, with retries.
    pub async fn upload(&self, path: &Path) -> UploadResult {
        let name = file_name(path);
        match tokio::fs::read(path).await {
            Ok(bytes) => self.upload_bytes(&name, bytes).await,
            Err(e) => UploadResult::Retryable(format!("read error: {e}")),
        }
    }

    /// Uploads bytes already read from disk under `file_name`, with retries.
    pub async fn upload_bytes(&self, file_name: &str, bytes: Vec<u8>) -> UploadResult {
        let bytes: Arc<[u8]> = bytes.into();
        let header = encode_filename(file_name);
        let mut last_error = String::new();

        for attempt in 1..=MAX_ATTEMPTS {
            match self.attempt(&header, bytes.clone()).await {
                Attempt::Done(result) => return result,
                Attempt::Retry(error) => {
                    tracing::warn!(file = file_name, attempt, %error, "upload attempt failed");
                    last_error = error;
                }
            }
            if attempt < MAX_ATTEMPTS {
                sleep(self.backoff_unit * 2u32.pow(attempt)).await;
            }
        }
        UploadResult::Retryable(format!("after {MAX_ATTEMPTS} attempts: {last_error}"))
    }

    async fn attempt(&self, header: &str, bytes: Arc<[u8]>) -> Attempt {
        let agent = self.agent.clone();
        let url = self.url.clone();
        let header = header.to_string();
        let sent = tokio::task::spawn_blocking(move || -> Result<(u16, String), String> {
            let mut response = agent
                .post(&url)
                .header("Content-Type", "application/octet-stream")
                .header("X-Filename", &header)
                .send(&bytes[..])
                .map_err(|e| format!("network error: {e}"))?;
            let status = response.status().as_u16();
            let body = response.body_mut().read_to_string().unwrap_or_default();
            Ok((status, body))
        })
        .await;

        match sent {
            Ok(Ok((status, body))) => classify(status, body),
            Ok(Err(error)) => Attempt::Retry(error),
            Err(join) => Attempt::Retry(format!("upload task failed: {join}")),
        }
    }
}

enum Attempt {
    Done(UploadResult),
    Retry(String),
}

fn classify(status: u16, body: String) -> Attempt {
    match status {
        200..=299 => Attempt::Done(UploadResult::Uploaded),
        409 => Attempt::Done(UploadResult::Duplicate),
        400..=499 => {
            let reason: String = body.trim().chars().take(MAX_REASON_CHARS).collect();
            let reason = if reason.is_empty() {
                format!("HTTP {status}")
            } else {
                reason
            };
            Attempt::Done(UploadResult::Rejected(reason))
        }
        _ => Attempt::Retry(format!("HTTP {status}: {}", body.trim())),
    }
}

/// Percent-encodes a file name for the `X-Filename` header: ASCII alphanumerics and
/// `.-_ ()` are kept, every other byte of the UTF-8 name becomes `%XX`.
pub fn encode_filename(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || b".-_ ()".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub(crate) fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}
