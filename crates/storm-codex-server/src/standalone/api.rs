//! `/api/standalone/*` routes, registered only in standalone mode (loopback-only server), and
//! the request filter that standalone mode applies to the whole app.
//!
//! Unauthenticated, like the rest of a single-user local install, but every request to any
//! route (API, pages, images, WebSocket) must come from this PC (403 otherwise):
//! - the `Host` must name the loopback server exactly (`127.0.0.1`, `localhost` or `[::1]`,
//!   with no port or the bound port): a DNS-rebound page (`evil.example` resolving to
//!   127.0.0.1) cannot read the stats or the status, mint an upload token or delete data;
//! - a request carrying an `Origin` other than the app's own (`http://` + a Host that passes
//!   the rule above), whatever its method, is refused: a page on another site posting to the
//!   local server, reading it with a CORS fetch, or opening `ws://127.0.0.1:<port>/ws` (a
//!   WebSocket upgrade is a GET and is not bound by the same-origin policy). Requests without
//!   `Origin` (navigations, `<img>`, OBS browser sources, the shell, the in-process uploader)
//!   are unaffected.

use std::path::PathBuf;

use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
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
}

fn error(code: StatusCode, message: String) -> Response {
    (code, Json(serde_json::json!({ "error": message }))).into_response()
}

/// The routes only exist when `standalone` is set; this is a defensive 404 otherwise.
fn not_found() -> Response {
    StatusCode::NOT_FOUND.into_response()
}

/// True for the `Origin` of the app's own pages: `http://` and an authority that passes
/// [`is_local_host`] (loopback name, no port or the bound port). Another local web app on
/// another port, `null` and any other site are refused.
fn is_app_origin(origin: &str, bound: Option<u16>) -> bool {
    origin
        .strip_prefix("http://")
        .is_some_and(|authority| is_local_host(authority, bound))
}

/// True for a `Host` (or HTTP/2 authority) naming the loopback server: exactly `127.0.0.1`,
/// `localhost` or `[::1]` (case-sensitive, no suffix), with no port or exactly `bound`.
fn is_local_host(host: &str, bound: Option<u16>) -> bool {
    let (name, port) = if let Some(rest) = host.strip_prefix('[') {
        let Some((inner, after)) = rest.split_once(']') else {
            return false;
        };
        (&host[..inner.len() + 2], after)
    } else {
        host.split_at(host.find(':').unwrap_or(host.len()))
    };
    if !matches!(name, "127.0.0.1" | "localhost" | "[::1]") {
        return false;
    }
    match port.strip_prefix(':') {
        None => port.is_empty(),
        Some(digits) => {
            !digits.is_empty()
                && digits.bytes().all(|b| b.is_ascii_digit())
                && digits.parse::<u16>().ok().is_some_and(|p| Some(p) == bound)
        }
    }
}

/// Middleware applied to the whole app in standalone mode (`app_router`): API, pages, images,
/// WebSocket. OBS browser sources send GETs without `Origin` and are unaffected.
pub(crate) async fn local_requests_only(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let bound = state.standalone.as_ref().and_then(|st| st.port());
    let host = match request.headers().get(header::HOST) {
        Some(value) => value.to_str().ok().map(str::to_owned),
        None => request.uri().authority().map(|a| a.as_str().to_owned()),
    };
    if !host.is_some_and(|h| is_local_host(&h, bound)) {
        return error(StatusCode::FORBIDDEN, "unexpected Host header".into());
    }
    let foreign = request
        .headers()
        .get(header::ORIGIN)
        .is_some_and(|o| !o.to_str().is_ok_and(|o| is_app_origin(o, bound)));
    if foreign {
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
    st.set_extra_folders(&saved);
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
    use super::{is_app_origin, is_local_host};

    #[test]
    fn local_hosts() {
        let bound = Some(8088);
        for ok in [
            "127.0.0.1",
            "localhost",
            "[::1]",
            "127.0.0.1:8088",
            "localhost:8088",
            "[::1]:8088",
        ] {
            assert!(is_local_host(ok, bound), "{ok}");
        }
        for bad in [
            "evil.com",
            "evil.com:8088",
            "127.0.0.1.evil.com",
            "localhost.evil.com",
            "localhost.evil.com:8088",
            "LOCALHOST:8088",
            "LocalHost",
            "127.0.0.1:9999",
            "localhost:",
            "localhost:+8088",
            "localhost:8088:1",
            "[::1]:9999",
            "[::1]x",
            "[::2]:8088",
            "[::1",
            "",
        ] {
            assert!(!is_local_host(bad, bound), "{bad}");
        }
        // port unknown (not bound yet): only a port-less Host passes
        assert!(is_local_host("127.0.0.1", None));
        assert!(!is_local_host("127.0.0.1:8088", None));
    }

    #[test]
    fn app_origins() {
        let bound = Some(8088);
        assert!(is_app_origin("http://127.0.0.1:8088", bound));
        assert!(is_app_origin("http://localhost:8088", bound));
        assert!(is_app_origin("http://[::1]:8088", bound));
        // another local web app, another site, opaque origins
        assert!(!is_app_origin("http://localhost:3000", bound));
        assert!(!is_app_origin("https://127.0.0.1:8088", bound));
        assert!(!is_app_origin("https://evil.example", bound));
        assert!(!is_app_origin("http://127.0.0.1.evil.example", bound));
        assert!(!is_app_origin("http://127.0.0.1:8088/", bound));
        assert!(!is_app_origin("null", bound));
    }
}
