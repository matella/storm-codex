//! Résultats de rang lus à l'écran de fin de partie — `POST /api/rank/result` (Aegis, token d'upload).
//! Spec : `docs/specs/2026-10-31-ocr-rang-fin-de-partie-design.md`. Le serveur ne devine rien : une
//! lecture `valide` doit être cohérente (total + requis pour la promotion = 1000 quand le requis existe).
use crate::upload::token_id;
use crate::AppState;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value as J};

/// Total + points requis pour la promotion : l'échelle de la page Play en Storm League.
pub const RANK_SCALE: i32 = 1000;
const MAX_TEXT: usize = 64;
const MAX_REASON: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Valide,
    Indisponible,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Self::Valide => "valide",
            Self::Indisponible => "indisponible",
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct RankResultBody {
    pub session_id: String,
    pub status: Status,
    #[serde(default)]
    pub rank_name: Option<String>,
    #[serde(default)]
    pub total_points: Option<i32>,
    #[serde(default)]
    pub promotion_required: Option<i32>,
    #[serde(default)]
    pub points_delta: Option<i32>,
    #[serde(default)]
    pub reason: Option<String>,
}

/// Code d'erreur (stable, lisible par Aegis) si le corps est incohérent ; `Ok` sinon.
pub fn validate(b: &RankResultBody) -> Result<(), &'static str> {
    let hex_session = b.session_id.len() == 64 && b.session_id.bytes().all(|c| c.is_ascii_hexdigit());
    if !hex_session {
        return Err("bad_session");
    }
    if b.rank_name.as_deref().is_some_and(|s| s.chars().count() > MAX_TEXT)
        || b.reason.as_deref().is_some_and(|s| s.chars().count() > MAX_REASON)
    {
        return Err("bad_text");
    }
    match b.status {
        Status::Indisponible => {
            let no_numbers = b.total_points.is_none() && b.promotion_required.is_none() && b.points_delta.is_none();
            if b.reason.is_none() {
                return Err("reason_required");
            }
            if !no_numbers {
                return Err("numbers_on_unavailable");
            }
            Ok(())
        }
        Status::Valide => {
            if b.rank_name.is_none() {
                return Err("rank_required");
            }
            let total = b.total_points.ok_or("total_required")?;
            if !(0..=RANK_SCALE).contains(&total) {
                return Err("bad_points");
            }
            if let Some(req) = b.promotion_required {
                if total + req != RANK_SCALE {
                    return Err("scale_mismatch");
                }
            }
            Ok(())
        }
    }
}

/// Match de la session, si le lobby courant est déjà lié à un match (`content_hash` = session,
/// `match_id` posé par `lobby::lier_match` à l'upload). `None` sinon : la lecture reste en attente.
pub fn linked_match(lobby: Option<(&str, Option<i64>)>, session: &str) -> Option<i64> {
    lobby.filter(|(hash, _)| *hash == session).and_then(|(_, m)| m)
}

/// Rattache les lectures en attente de la session au match `match_id` (appelé quand le lobby est lié).
/// Renvoie le nombre de lignes rattachées ; sans effet sur celles déjà liées.
pub async fn attach_to_match(db: &sqlx::PgPool, session: &str, match_id: i64) -> Result<u64, sqlx::Error> {
    let done = sqlx::query("UPDATE ranked_results SET match_id = $2 WHERE session_id = $1 AND match_id IS NULL")
        .bind(session.to_ascii_lowercase())
        .bind(match_id)
        .execute(db)
        .await?;
    Ok(done.rows_affected())
}

/// POST /api/rank/result — corps JSON `RankResultBody`. 201 `{id}` ; 401 sans token valide ;
/// 422 `{error}` si le corps est incohérent (rien n'est écrit).
pub async fn post_result(State(s): State<AppState>, headers: HeaderMap, Json(b): Json<RankResultBody>) -> (StatusCode, Json<J>) {
    let Some(token) = token_id(&s.db, &headers).await else {
        return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "unauthorized" })));
    };
    if let Err(code) = validate(&b) {
        return (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "error": code })));
    }
    let session = b.session_id.to_ascii_lowercase();
    let match_id = {
        let lobby = s.lobby.read().await;
        linked_match(lobby.as_ref().map(|l| (l.content_hash.as_str(), l.match_id)), &session)
    };
    let inserted: Result<i64, sqlx::Error> = sqlx::query_scalar(
        "INSERT INTO ranked_results (session_id, status, rank_name, total_points, promotion_required, points_delta, reason, token_id, match_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING id",
    )
    .bind(&session)
    .bind(b.status.as_str())
    .bind(&b.rank_name)
    .bind(b.total_points)
    .bind(b.promotion_required)
    .bind(b.points_delta)
    .bind(&b.reason)
    .bind(token)
    .bind(match_id)
    .fetch_one(&s.db)
    .await;
    match inserted {
        Ok(id) => (StatusCode::CREATED, Json(json!({ "id": id }))),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "db" }))),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn valid(total: i32, req: Option<i32>) -> RankResultBody {
        RankResultBody {
            session_id: "a".repeat(64),
            status: Status::Valide,
            rank_name: Some("Diamond 1".into()),
            total_points: Some(total),
            promotion_required: req,
            points_delta: None,
            reason: None,
        }
    }

    #[test]
    fn scale_1000_is_checked_when_promotion_is_shown() {
        // lecture observée : 535 + 465 = 1000
        assert_eq!(validate(&valid(535, Some(465))), Ok(()));
        assert_eq!(validate(&valid(535, Some(460))), Err("scale_mismatch"));
        // pas de seuil affiché (ex. Grandmaster) : pas de contrôle croisé
        assert_eq!(validate(&valid(535, None)), Ok(()));
    }

    #[test]
    fn valid_result_needs_rank_and_total_in_range() {
        let mut b = valid(535, Some(465));
        b.rank_name = None;
        assert_eq!(validate(&b), Err("rank_required"));
        assert_eq!(validate(&valid(1001, None)), Err("bad_points"));
        assert_eq!(validate(&valid(-1, None)), Err("bad_points"));
    }

    #[test]
    fn unavailable_has_reason_and_no_numbers() {
        let mut b = RankResultBody {
            session_id: "b".repeat(64),
            status: Status::Indisponible,
            rank_name: None,
            total_points: None,
            promotion_required: None,
            points_delta: None,
            reason: None,
        };
        assert_eq!(validate(&b), Err("reason_required"));
        b.reason = Some("titre non lu".into());
        assert_eq!(validate(&b), Ok(()));
        b.points_delta = Some(12);
        assert_eq!(validate(&b), Err("numbers_on_unavailable"));
    }

    #[test]
    fn linked_match_only_when_the_lobby_is_this_session() {
        let me = "c".repeat(64);
        assert_eq!(linked_match(Some((&me, Some(7))), &me), Some(7));
        assert_eq!(linked_match(Some((&me, None)), &me), None);
        assert_eq!(linked_match(Some(("d".repeat(64).as_str(), Some(7))), &me), None);
        assert_eq!(linked_match(None, &me), None);
    }

    #[test]
    fn session_must_be_sha256_hex() {
        let mut b = valid(535, Some(465));
        b.session_id = "xyz".into();
        assert_eq!(validate(&b), Err("bad_session"));
        b.session_id = "g".repeat(64);
        assert_eq!(validate(&b), Err("bad_session"));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod db_tests {
    use super::*;
    use crate::api_tests::test_state;
    use crate::upload::sha256_hex;
    use axum::http::HeaderValue;

    /// Token d'upload connu (hash en base, clair dans l'en-tête) ; renvoie le clair.
    async fn token(db: &sqlx::PgPool) -> String {
        let clear = format!("rank-test-{}", uuid::Uuid::new_v4());
        sqlx::query("INSERT INTO upload_tokens (name, token_hash) VALUES ($1, $2)")
            .bind(format!("test-rank-{}", uuid::Uuid::new_v4()))
            .bind(sha256_hex(clear.as_bytes()))
            .execute(db)
            .await
            .unwrap();
        clear
    }

    fn bearer(t: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("authorization", HeaderValue::from_str(&format!("Bearer {t}")).unwrap());
        h
    }

    fn session() -> String {
        format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
    }

    #[tokio::test]
    async fn post_result_needs_token_and_stores_valid_reading() {
        let Some(state) = test_state(None).await else { return };
        let body = || RankResultBody {
            session_id: session(),
            status: Status::Valide,
            rank_name: Some("Diamond 1".into()),
            total_points: Some(535),
            promotion_required: Some(465),
            points_delta: Some(486),
            reason: None,
        };
        let sid = body().session_id;
        let (st, _) = post_result(State(state.clone()), HeaderMap::new(), Json(body())).await;
        assert_eq!(st, StatusCode::UNAUTHORIZED);

        let clear = token(&state.db).await;
        let mut b = body();
        b.session_id = sid.clone();
        let (st, v) = post_result(State(state.clone()), bearer(&clear), Json(b)).await;
        assert_eq!(st, StatusCode::CREATED, "{v:?}");
        let row: (String, i32, i32) = sqlx::query_as(
            "SELECT status, total_points, promotion_required FROM ranked_results WHERE session_id = $1",
        )
        .bind(&sid)
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(row, ("valide".into(), 535, 465));
    }

    #[tokio::test]
    async fn post_result_rejects_incoherent_body_and_writes_nothing() {
        let Some(state) = test_state(None).await else { return };
        let clear = token(&state.db).await;
        let sid = session();
        let b = RankResultBody {
            session_id: sid.clone(),
            status: Status::Valide,
            rank_name: Some("Diamond 1".into()),
            total_points: Some(535),
            promotion_required: Some(460),
            points_delta: None,
            reason: None,
        };
        let (st, v) = post_result(State(state.clone()), bearer(&clear), Json(b)).await;
        assert_eq!((st, v.0["error"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("scale_mismatch")));
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM ranked_results WHERE session_id = $1")
            .bind(&sid)
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn pending_reading_attaches_once_to_its_match() {
        let Some(state) = test_state(None).await else { return };
        let clear = token(&state.db).await;
        let sid = session();
        let b = RankResultBody {
            session_id: sid.clone(),
            status: Status::Valide,
            rank_name: Some("Diamond 1".into()),
            total_points: Some(535),
            promotion_required: Some(465),
            points_delta: None,
            reason: None,
        };
        let (st, _) = post_result(State(state.clone()), bearer(&clear), Json(b)).await;
        assert_eq!(st, StatusCode::CREATED);
        let match_id = crate::players::testutil::insert_match(&state.db, None, &[]).await;
        assert_eq!(attach_to_match(&state.db, &sid, match_id).await.unwrap(), 1);
        assert_eq!(attach_to_match(&state.db, &sid, match_id).await.unwrap(), 0);
        let got: Option<i64> = sqlx::query_scalar("SELECT match_id FROM ranked_results WHERE session_id = $1")
            .bind(&sid)
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(got, Some(match_id));
    }
}
