//! Shared test helpers: a tiny upload server and replay file builders.
#![allow(dead_code, clippy::expect_used)]

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use axum::body::Bytes;
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::Router;

/// One request received by the test server.
#[derive(Debug, Clone)]
pub struct Hit {
    pub mode: String,
    /// Raw `X-Filename` header, exactly as sent.
    pub filename: String,
    pub body_len: usize,
    /// Raw `Authorization` header, if any.
    pub authorization: Option<String>,
}

pub struct TestServer {
    addr: String,
    pub hits: Arc<Mutex<Vec<Hit>>>,
}

impl TestServer {
    /// Base URL whose `/api/upload` answers according to `mode`:
    /// any numeric status (`200`, `409`, `422`, `500`, ...), or `mixed`
    /// (422 when the file name starts with `bad`, 503 for `slow`, else 200).
    pub fn base(&self, mode: &str) -> String {
        format!("http://{}/{mode}", self.addr)
    }

    pub fn hits(&self) -> Vec<Hit> {
        self.hits.lock().expect("hits lock").clone()
    }

    pub fn filenames(&self) -> Vec<String> {
        self.hits().into_iter().map(|h| h.filename).collect()
    }
}

pub const REJECT_BODY: &str =
    r#"{"error":"not_custom","message":"only custom games are accepted"}"#;

async fn handle(
    UrlPath(mode): UrlPath<String>,
    State(hits): State<Arc<Mutex<Vec<Hit>>>>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, String) {
    let filename = headers
        .get("x-filename")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let content_type_ok = headers.get("content-type").and_then(|v| v.to_str().ok())
        == Some("application/octet-stream");
    let authorization = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    hits.lock().expect("hits lock").push(Hit {
        mode: mode.clone(),
        filename: filename.clone(),
        body_len: body.len(),
        authorization,
    });
    if !content_type_ok {
        return (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "bad content type".into(),
        );
    }
    match mode.as_str() {
        "200" => (StatusCode::OK, r#"{"status":"ok"}"#.into()),
        "409" => (StatusCode::CONFLICT, r#"{"error":"duplicate"}"#.into()),
        "422" => (StatusCode::UNPROCESSABLE_ENTITY, REJECT_BODY.into()),
        // storm-codex-server's answer for a replay it stored but could not turn into a match.
        "parse_failed" => (
            StatusCode::OK,
            r#"{"status":"parse_failed","error_class":"too_old"}"#.into(),
        ),
        "parse_failed_no_class" => (StatusCode::OK, r#"{"status":"parse_failed"}"#.into()),
        "mixed" if filename.starts_with("slow") => (StatusCode::SERVICE_UNAVAILABLE, "busy".into()),
        "mixed" if filename.starts_with("bad") => {
            (StatusCode::UNPROCESSABLE_ENTITY, REJECT_BODY.into())
        }
        "mixed" => (StatusCode::OK, r#"{"status":"ok"}"#.into()),
        // Any other numeric mode answers with that status.
        code => match code
            .parse::<u16>()
            .ok()
            .and_then(|c| StatusCode::from_u16(c).ok())
        {
            Some(status) => (status, format!("status {code}")),
            None => (StatusCode::INTERNAL_SERVER_ERROR, "boom".into()),
        },
    }
}

/// Starts the test server on a random local port.
pub async fn spawn_server() -> TestServer {
    let hits: Arc<Mutex<Vec<Hit>>> = Arc::default();
    let app = Router::new()
        .route("/{mode}/api/upload", post(handle))
        .with_state(hits.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let addr = listener.local_addr().expect("local addr").to_string();
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    TestServer { addr, hits }
}

/// Writes a fake replay whose mtime is `age` in the past (backfill skips files touched just now).
pub fn write_replay(dir: &Path, name: &str, age: Duration) -> PathBuf {
    std::fs::create_dir_all(dir).expect("create replay dir");
    let path = dir.join(name);
    let mut f = File::create(&path).expect("create replay");
    f.write_all(format!("MPQ fake replay {name}").as_bytes())
        .expect("write replay");
    f.set_modified(SystemTime::now() - age).expect("set mtime");
    path
}
