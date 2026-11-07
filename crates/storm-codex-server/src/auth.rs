//! Auth admin partagée (`admin.rs` + `manage.rs`) : Bearer contre `ADMIN_TOKEN`.
//! `ADMIN_TOKEN` absent/vide = **mode ouvert** (auto-hébergement local, LAN/Tailscale).

use axum::http::HeaderMap;
use sha2::{Digest, Sha256};

/// `true` si la requête est autorisée à écrire : mode ouvert (pas de token configuré)
/// ou `Authorization: Bearer <token>` exact (espaces tolérés autour du token).
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
    // On compare les empreintes SHA-256 octet par octet sans sortie anticipée : la durée de la
    // comparaison ne révèle pas la longueur du préfixe correct du token.
    let (a, b) = (
        Sha256::digest(given.as_bytes()),
        Sha256::digest(token.as_bytes()),
    );
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// Préfixes des écritures de la régie (cast) et du simulateur de draft : elles changent ce qui
/// passe à l'antenne dans OBS. Avec `ADMIN_TOKEN`, elles exigent le Bearer admin comme le reste
/// de l'API d'écriture ; en mode ouvert, rien ne change. Les lectures (GET/HEAD) restent libres :
/// les overlays OBS n'envoient pas de token.
const GUARDED_PREFIXES: [&str; 2] = ["/api/cast", "/api/draft"];

pub fn is_guarded_write(method: &axum::http::Method, path: &str) -> bool {
    use axum::http::Method;
    let read = matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS);
    !read
        && GUARDED_PREFIXES
            .iter()
            .any(|p| path == *p || path.starts_with(&format!("{p}/")))
}

/// Middleware : `401 {error}` sur une écriture cast/draft sans Bearer admin valide.
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
        // schéma non-Bearer refusé
        assert!(!is_admin(&headers(Some("Basic s3cret")), Some("s3cret")));
        // le token seul, sans le préfixe Bearer, est refusé
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
        // préfixe voisin non concerné
        assert!(!is_guarded_write(&Method::POST, "/api/castle"));
        assert!(!is_guarded_write(&Method::POST, "/api/upload"));
    }
}
