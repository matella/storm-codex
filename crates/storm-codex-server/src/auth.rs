//! Auth admin partagée (`admin.rs` + `manage.rs`) : Bearer contre `ADMIN_TOKEN`.
//! `ADMIN_TOKEN` absent/vide = **mode ouvert** (auto-hébergement local, LAN/Tailscale).

use axum::http::HeaderMap;

/// `true` si la requête est autorisée à écrire : mode ouvert (pas de token configuré)
/// ou `Authorization: Bearer <token>` exact (espaces tolérés autour du token).
pub fn is_admin(headers: &HeaderMap, configured: Option<&str>) -> bool {
    let Some(token) = configured else { return true };
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(str::trim)
        == Some(token)
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
}
