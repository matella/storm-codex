//! Shared admin auth (`admin.rs` + `manage.rs`): Bearer against `ADMIN_TOKEN`.
//! `ADMIN_TOKEN` missing/empty = **open mode** (local self-hosting, LAN/Tailscale).

use axum::http::HeaderMap;
use sha2::{Digest, Sha256};

/// `true` if the request is allowed to write: open mode (no token configured)
/// or exact `Authorization: Bearer <token>` (spaces tolerated around the token).
pub fn is_admin(headers: &HeaderMap, configured: Option<&str>) -> bool {
    let Some(token) = configured else { return true };
    let Some(given) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(str::trim)
    else {
        return false;
    };
    // We compare the SHA-256 digests byte by byte without early exit: the duration of the
    // comparison does not reveal the length of the token's correct prefix.
    let (a, b) = (
        Sha256::digest(given.as_bytes()),
        Sha256::digest(token.as_bytes()),
    );
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// Prefixes of the control room (cast) and draft simulator writes: they change what
/// goes on air in OBS. With `ADMIN_TOKEN`, they require the admin Bearer like the rest
/// of the write API; in open mode, nothing changes. Reads (GET/HEAD) stay free:
/// the OBS overlays do not send a token.
const GUARDED_PREFIXES: [&str; 3] = ["/api/cast", "/api/draft", "/api/lobby"];

pub fn is_guarded_write(method: &axum::http::Method, path: &str) -> bool {
    use axum::http::Method;
    let read = matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS);
    // `POST /api/lobby` = companion ingestion, already authenticated by an upload token.
    let ingest = *method == Method::POST && path == "/api/lobby";
    !read
        && !ingest
        && GUARDED_PREFIXES
            .iter()
            .any(|p| path == *p || path.starts_with(&format!("{p}/")))
}

/// Middleware: `401 {error}` on a cast/draft write without a valid admin Bearer.
pub async fn guard_writes(
    axum::extract::State(state): axum::extract::State<crate::AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if is_guarded_write(request.method(), request.uri().path())
        && !is_admin(request.headers(), state.cfg.admin_token.as_deref())
    {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({"error": "admin token requis"})),
        )
            .into_response();
    }
    next.run(request).await
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(auth: Option<&str>) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Some(v) = auth {
            h.insert(
                axum::http::header::AUTHORIZATION,
                HeaderValue::from_str(v).unwrap(),
            );
        }
        h
    }

    #[test]
    fn mode_ouvert_sans_token_configure() {
        assert!(is_admin(&headers(None), None));
        assert!(is_admin(&headers(Some("Bearer nimporte")), None));
    }

    #[test]
    fn bon_token_accepte_espaces_toleres() {
        assert!(is_admin(&headers(Some("Bearer s3cret")), Some("s3cret")));
        assert!(is_admin(&headers(Some("Bearer  s3cret ")), Some("s3cret")));
    }

    #[test]
    fn mauvais_ou_absent_refuse() {
        assert!(!is_admin(&headers(Some("Bearer faux")), Some("s3cret")));
        assert!(!is_admin(&headers(None), Some("s3cret")));
        // non-Bearer scheme refused
        assert!(!is_admin(&headers(Some("Basic s3cret")), Some("s3cret")));
        // the token alone, without the Bearer prefix, is refused
        assert!(!is_admin(&headers(Some("s3cret")), Some("s3cret")));
    }

    #[test]
    fn ecritures_cast_et_draft_gardees_lectures_libres() {
        use axum::http::Method;
        assert!(is_guarded_write(&Method::POST, "/api/cast/scene"));
        assert!(is_guarded_write(&Method::DELETE, "/api/cast/games/2"));
        assert!(is_guarded_write(&Method::PUT, "/api/cast/teams"));
        assert!(is_guarded_write(&Method::POST, "/api/draft/action"));
        assert!(is_guarded_write(&Method::POST, "/api/draft"));
        assert!(!is_guarded_write(&Method::GET, "/api/cast"));
        assert!(!is_guarded_write(&Method::GET, "/api/cast/logo/blue"));
        assert!(!is_guarded_write(&Method::GET, "/api/draft/replay"));
        // neighbouring prefix not concerned
        assert!(!is_guarded_write(&Method::POST, "/api/castle"));
        assert!(!is_guarded_write(&Method::POST, "/api/upload"));
        // lobby: manual corrections guarded, ingestion (upload token) not
        assert!(is_guarded_write(&Method::POST, "/api/lobby/hero"));
        assert!(is_guarded_write(&Method::DELETE, "/api/lobby"));
        assert!(!is_guarded_write(&Method::POST, "/api/lobby"));
    }
}
