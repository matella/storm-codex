//! `/api/standalone/*` routes, registered only in standalone mode (loopback-only server).
//!
//! Unauthenticated, like the rest of a single-user local install, but every write must come
//! from the app itself: a request carrying a non-loopback `Origin` (a web page on another site
//! posting to the local server) is refused with 403.

use std::path::PathBuf;

use axum::extract::{Request, State};
use axum::http::{header, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;

use super::{resolve_folders, save_extra_folders, Command, Phase};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/standalone/status", get(status))
        .route("/api/standalone/folders", put(put_folders))
        .route("/api/standalone/rescan", post(rescan))
        .route("/api/standalone/shutdown", post(shutdown))
        .route_layer(axum::middleware::from_fn(same_origin_writes))
}

fn error(code: StatusCode, message: String) -> Response {
    (code, Json(serde_json::json!({ "error": message }))).into_response()
}

/// The routes only exist when `standalone` is set; this is a defensive 404 otherwise.
fn not_found() -> Response {
    StatusCode::NOT_FOUND.into_response()
}

/// True for an `Origin` served from the loopback interface (the app's own pages).
fn is_loopback_origin(origin: &str) -> bool {
    let Ok(uri) = origin.parse::<axum::http::Uri>() else {
        return false;
    };
    matches!(uri.host(), Some("127.0.0.1" | "localhost" | "[::1]"))
}

async fn same_origin_writes(request: Request, next: Next) -> Response {
    let read = matches!(*request.method(), Method::GET | Method::HEAD);
    let foreign = request
        .headers()
        .get(header::ORIGIN)
        .is_some_and(|o| !o.to_str().is_ok_and(is_loopback_origin));
    if !read && foreign {
        return error(
            StatusCode::FORBIDDEN,
            "cross-origin requests are not allowed".into(),
        );
    }
    next.run(request).await
}

/// GET /api/standalone/status
async fn status(State(state): State<AppState>) -> Response {
    match state.standalone {
        Some(st) => Json(st.status()).into_response(),
        None => not_found(),
    }
}

#[derive(Deserialize)]
struct FoldersBody {
    folders: Vec<String>,
}

/// PUT /api/standalone/folders — replaces the extra folders, then restarts the watcher and
/// rescans. Every entry must be an existing directory (400 otherwise); only directory listings
/// are read, never file contents.
async fn put_folders(State(state): State<AppState>, Json(body): Json<FoldersBody>) -> Response {
    let Some(st) = state.standalone.clone() else {
        return not_found();
    };
    let mut extras = Vec::with_capacity(body.folders.len());
    for folder in &body.folders {
        let path = PathBuf::from(folder.trim());
        if folder.trim().is_empty() || !path.is_dir() {
            return error(
                StatusCode::BAD_REQUEST,
                format!("not an existing folder: {folder:?}"),
            );
        }
        extras.push(path);
    }
    let saved: Vec<String> = extras
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    if let Err(e) = save_extra_folders(&state.db, &saved).await {
        tracing::error!(error = %e, "saving the replay folders");
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "could not save the folders".into(),
        );
    }
    let resolver = st.clone();
    let folders = match tokio::task::spawn_blocking(move || {
        resolve_folders(resolver.detect(), &extras)
    })
    .await
    {
        Ok(folders) => folders,
        Err(e) => {
            tracing::error!(error = %e, "resolving the replay folders");
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "could not read the folders".into(),
            );
        }
    };
    st.set_folders(&folders);
    st.set_phase(if folders.is_empty() {
        Phase::NoFolders
    } else {
        Phase::Importing
    });
    if !st.send(Command::SetFolders(folders)) {
        tracing::warn!("folders saved, but the import task is not running");
    }
    Json(st.status()).into_response()
}

/// POST /api/standalone/rescan — re-runs the disk gate and the backfill.
async fn rescan(State(state): State<AppState>) -> Response {
    match state.standalone {
        Some(st) => {
            if !st.send(Command::Rescan) {
                tracing::warn!("rescan requested, but the import task is not running");
            }
            StatusCode::ACCEPTED.into_response()
        }
        None => not_found(),
    }
}

/// POST /api/standalone/shutdown — graceful stop, same path as Ctrl-C (used by the desktop
/// shell before it stops Postgres).
async fn shutdown(State(state): State<AppState>) -> Response {
    match state.standalone {
        Some(st) => {
            tracing::info!("shutdown requested over HTTP");
            st.request_shutdown();
            StatusCode::ACCEPTED.into_response()
        }
        None => not_found(),
    }
}

#[cfg(test)]
mod tests {
    use super::is_loopback_origin;

    #[test]
    fn loopback_origins() {
        assert!(is_loopback_origin("http://127.0.0.1:8088"));
        assert!(is_loopback_origin("http://localhost:3000"));
        assert!(is_loopback_origin("http://[::1]:8088"));
        assert!(!is_loopback_origin("https://evil.example"));
        assert!(!is_loopback_origin("http://127.0.0.1.evil.example"));
        assert!(!is_loopback_origin("null"));
    }
}
