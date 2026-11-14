//! Upload client for `POST {base}/api/upload` (protocol of `client-rs`).
//!
//! The request body is the raw replay (`Content-Type: application/octet-stream`); the file
//! name travels percent-encoded in `X-Filename`. Answers map to [`UploadResult`]:
//! 2xx → `Uploaded` (except a body `{"status":"parse_failed", "error_class": c}`, the server's
//! answer for a stored replay that gave no match → `Rejected("not imported: c")`), 409 →
//! `Duplicate`, content errors (400, 413, 415, 422) → `Rejected(body)` (never retried); network error, 5xx and any other 4xx (auth, config,
//! rate limit) → retried with a `2^n` backoff, `Retryable` once [`MAX_ATTEMPTS`] are spent.
//! A `Retryable` file must not be recorded in the ledger. An optional bearer token
//! ([`Uploader::with_token`]) is sent as `Authorization: Bearer <token>`.

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

impl std::fmt::Debug for Uploader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the token.
        f.debug_struct("Uploader")
            .field("url", &self.url)
            .field("backoff_unit", &self.backoff_unit)
            .field("token", &self.token.as_ref().map(|_| "<redacted>"))
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub struct Uploader {
    url: String,
    agent: Arc<ureq::Agent>,
    backoff_unit: Duration,
    token: Option<Arc<str>>,
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
            token: None,
        }
    }

    /// Sends `Authorization: Bearer <token>` with every upload.
    pub fn with_token(mut self, token: String) -> Self {
        self.token = Some(token.into());
        self
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
        let token = self.token.clone();
        let sent = tokio::task::spawn_blocking(move || -> Result<(u16, String), String> {
            let mut request = agent
                .post(&url)
                .header("Content-Type", "application/octet-stream")
                .header("X-Filename", &header);
            if let Some(token) = token {
                request = request.header("Authorization", &format!("Bearer {token}"));
            }
            let mut response = request
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

/// 4xx statuses that mean "this file's content is refused" (bad request, too large,
/// unsupported media type, unprocessable replay). Every other 4xx (401, 403, 404, 408,
/// 429, ...) is an auth/config/transient problem: retried, never recorded as rejected, so a
/// misconfigured server cannot poison the whole archive.
const CONTENT_REJECTIONS: [u16; 4] = [400, 413, 415, 422];

fn classify(status: u16, body: String) -> Attempt {
    match status {
        200..=299 => Attempt::Done(match parse_failed_class(&body) {
            Some(class) => UploadResult::Rejected(format!("not imported: {class}")),
            None => UploadResult::Uploaded,
        }),
        409 => Attempt::Done(UploadResult::Duplicate),
        s if CONTENT_REJECTIONS.contains(&s) => {
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

/// The error class of a storm-codex-server `200 {"status":"parse_failed", "error_class": ...}`
/// answer (`"parse_failed"` when the class is missing): the file was stored but no match came
/// out of it (too old, unsupported mode, stats failure...). Any other body is a success.
fn parse_failed_class(body: &str) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct Answer {
        status: Option<String>,
        error_class: Option<String>,
    }
    let answer: Answer = serde_json::from_str(body).ok()?;
    if answer.status.as_deref() != Some("parse_failed") {
        return None;
    }
    let class: String = answer
        .error_class
        .unwrap_or_else(|| "parse_failed".into())
        .chars()
        .take(MAX_REASON_CHARS)
        .collect();
    Some(class)
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
