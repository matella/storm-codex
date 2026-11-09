//! Rank results read on the end-of-game screen — `POST /api/rank/result` (Aegis, upload token).
//! Spec: `docs/specs/2026-10-31-ocr-rang-fin-de-partie-design.md`. The server guesses nothing: a
//! `valide` reading must be consistent (total + required for promotion = 1000 when the requirement exists).
//! A single reading per session: the last one wins, and keeps the link already acquired.
use crate::upload::token_id;
use crate::AppState;
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value as J};

/// Total + points required for promotion: the scale of the Play page in Storm League.
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

/// JSON body of the route. The session is not in it: it comes from the `X-Session` header.
#[derive(Debug, Deserialize)]
pub struct RankResultBody {
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

/// Session from `X-Session`: hex SHA-256 of the battlelobby (64 characters), normalised to lowercase.
pub fn session_from_headers(headers: &HeaderMap) -> Result<String, &'static str> {
    let raw = headers
        .get("x-session")
        .ok_or("missing_session")?
        .to_str()
        .map_err(|_| "bad_session")?;
    let s = raw.trim();
    if s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit()) {
        Ok(s.to_ascii_lowercase())
    } else {
        Err("bad_session")
    }
}

/// A provided but empty text (or only spaces) is refused: it says nothing.
fn blank(s: &Option<String>) -> bool {
    s.as_deref().is_some_and(|t| t.trim().is_empty())
}

/// Error code (stable, readable by Aegis) if the body is inconsistent; `Ok` otherwise.
pub fn validate(b: &RankResultBody) -> Result<(), &'static str> {
    if b.rank_name
        .as_deref()
        .is_some_and(|s| s.chars().count() > MAX_TEXT)
        || b.reason
            .as_deref()
            .is_some_and(|s| s.chars().count() > MAX_REASON)
    {
        return Err("bad_text");
    }
    if let Some(d) = b.points_delta {
        if d.unsigned_abs() > RANK_SCALE as u32 {
            return Err("bad_points");
        }
    }
    match b.status {
        Status::Indisponible => {
            if blank(&b.reason) || b.reason.is_none() {
                return Err("reason_required");
            }
            if b.total_points.is_some()
                || b.promotion_required.is_some()
                || b.points_delta.is_some()
            {
                return Err("numbers_on_unavailable");
            }
            if b.rank_name.is_some() {
                return Err("rank_on_unavailable");
            }
            Ok(())
        }
        Status::Valide => {
            if blank(&b.rank_name) || b.rank_name.is_none() {
                return Err("rank_required");
            }
            let total = b.total_points.ok_or("total_required")?;
            if !(0..=RANK_SCALE).contains(&total) {
                return Err("bad_points");
            }
            if let Some(req) = b.promotion_required {
                if total.checked_add(req) != Some(RANK_SCALE) {
                    return Err("scale_mismatch");
                }
            }
            Ok(())
        }
    }
}

/// Match of the session, if the current lobby is already linked to a match (`content_hash` = session,
/// `match_id` set by `lobby::lier_match` at upload). `None` otherwise: the reading stays pending.
pub fn linked_match(lobby: Option<(&str, Option<i64>)>, session: &str) -> Option<i64> {
    lobby
        .filter(|(hash, _)| *hash == session)
        .and_then(|(_, m)| m)
}

/// Attaches the pending readings of the session to the match `match_id` (lobby linked at upload) and
/// remembers the match fingerprint, to find the link again after a re-process. Idempotent.
pub async fn attach_to_match(
    db: &sqlx::PgPool,
    session: &str,
    match_id: i64,
) -> Result<u64, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE ranked_results
         SET match_id = $2, match_fingerprint = (SELECT fingerprint FROM matches WHERE id = $2)
         WHERE session_id = $1 AND match_id IS NULL",
    )
    .bind(session.to_ascii_lowercase())
    .bind(match_id)
    .execute(db)
    .await?;
    Ok(done.rows_affected())
}

/// Attaches the readings linked to the fingerprint `fingerprint` to the match `match_id`. Called at each
/// projection of a game: a re-process deletes and recreates the match (new id), and `ON DELETE SET NULL`
/// detaches the readings; the fingerprint, however, is stable and allows attaching them again.
pub async fn attach_by_fingerprint(
    db: &sqlx::PgPool,
    fingerprint: &str,
    match_id: i64,
) -> Result<u64, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE ranked_results SET match_id = $2 WHERE match_fingerprint = $1 AND match_id IS NULL",
    )
    .bind(fingerprint)
    .bind(match_id)
    .execute(db)
    .await?;
    Ok(done.rows_affected())
}

fn reply(code: StatusCode, body: J) -> (StatusCode, Json<J>) {
    (code, Json(body))
}

/// POST /api/rank/result — `X-Session` header, `RankResultBody` JSON body. Spec rule: the last
/// **valide** reading of the session wins; an `indisponible` reading never overwrites a `valide` one.
/// 201 `{id}` (reading recorded); 200 `{id, kept: true}` (`indisponible` reading ignored, a `valide` one
/// exists); 401 without a valid token; 422 `{error}` if the session or the body is inconsistent (nothing is written);
/// 500 `{error: "db"}` if the database fails. The token is checked before any processing of the body.
pub async fn post_result(
    State(s): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<J>) {
    let Some(token) = token_id(&s.db, &headers).await else {
        return reply(StatusCode::UNAUTHORIZED, json!({ "error": "unauthorized" }));
    };
    let session = match session_from_headers(&headers) {
        Ok(s) => s,
        Err(code) => return reply(StatusCode::UNPROCESSABLE_ENTITY, json!({ "error": code })),
    };
    let b: RankResultBody = match serde_json::from_slice(&body) {
        Ok(b) => b,
        Err(_) => {
            return reply(
                StatusCode::UNPROCESSABLE_ENTITY,
                json!({ "error": "bad_json" }),
            )
        }
    };
    if let Err(code) = validate(&b) {
        return reply(StatusCode::UNPROCESSABLE_ENTITY, json!({ "error": code }));
    }

    // The lobby read lock stays held until the commit: `run_parse` cannot link the lobby
    // between the `match_id` check and the write (otherwise the reading would stay orphaned).
    let lobby = s.lobby.read().await;
    let linked = linked_match(
        lobby
            .as_ref()
            .map(|l| (l.content_hash.as_str(), l.match_id)),
        &session,
    );
    let db_error = |e: sqlx::Error| {
        tracing::error!("rank insert: {e}");
        reply(StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": "db" }))
    };
    // A linked match that no longer exists (re-process in progress or finished) must not be set: broken foreign key.
    let (match_id, fingerprint) = match linked {
        Some(m) => {
            match sqlx::query_scalar::<_, String>("SELECT fingerprint FROM matches WHERE id = $1")
                .bind(m)
                .fetch_optional(&s.db)
                .await
            {
                Ok(Some(fp)) => (Some(m), Some(fp)),
                Ok(None) => (None, None),
                Err(e) => return db_error(e),
            }
        }
        None => (None, None),
    };
    // Atomic upsert on the session (UNIQUE constraint): two simultaneous POSTs do not create two rows.
    // The update only applies if the new reading is `valide`, or if the existing one is not.
    let written: Result<Option<i64>, sqlx::Error> = sqlx::query_scalar(
        "INSERT INTO ranked_results
            (session_id, status, rank_name, total_points, promotion_required, points_delta, reason, token_id, match_id, match_fingerprint)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         ON CONFLICT (session_id) DO UPDATE SET
            status = EXCLUDED.status, rank_name = EXCLUDED.rank_name, total_points = EXCLUDED.total_points,
            promotion_required = EXCLUDED.promotion_required, points_delta = EXCLUDED.points_delta,
            reason = EXCLUDED.reason, token_id = EXCLUDED.token_id, read_at = now(),
            match_id = COALESCE(EXCLUDED.match_id, ranked_results.match_id),
            match_fingerprint = COALESCE(EXCLUDED.match_fingerprint, ranked_results.match_fingerprint)
         WHERE EXCLUDED.status = 'valide' OR ranked_results.status <> 'valide'
         RETURNING id",
    )
    .bind(&session)
    .bind(b.status.as_str())
    .bind(b.rank_name.as_deref().map(str::trim))
    .bind(b.total_points)
    .bind(b.promotion_required)
    .bind(b.points_delta)
    .bind(b.reason.as_deref().map(str::trim))
    .bind(token)
    .bind(match_id)
    .bind(fingerprint)
    .fetch_optional(&s.db)
    .await;
    drop(lobby);
    match written {
        Ok(Some(id)) => reply(StatusCode::CREATED, json!({ "id": id })),
        // indisponible received after a valide: nothing is written, the valide stays
        Ok(None) => {
            let id: Result<i64, sqlx::Error> =
                sqlx::query_scalar("SELECT id FROM ranked_results WHERE session_id = $1")
                    .bind(&session)
                    .fetch_one(&s.db)
                    .await;
            match id {
                Ok(id) => reply(StatusCode::OK, json!({ "id": id, "kept": true })),
                Err(e) => db_error(e),
            }
        }
        Err(e) => db_error(e),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn valid(total: i32, req: Option<i32>) -> RankResultBody {
        RankResultBody {
            status: Status::Valide,
            rank_name: Some("Diamond 1".into()),
            total_points: Some(total),
            promotion_required: req,
            points_delta: None,
            reason: None,
        }
    }

    fn unavailable(reason: Option<&str>) -> RankResultBody {
        RankResultBody {
            status: Status::Indisponible,
            rank_name: None,
            total_points: None,
            promotion_required: None,
            points_delta: None,
            reason: reason.map(Into::into),
        }
    }

    #[test]
    fn scale_1000_is_checked_when_promotion_is_shown() {
        // observed reading: 535 + 465 = 1000
        assert_eq!(validate(&valid(535, Some(465))), Ok(()));
        assert_eq!(validate(&valid(535, Some(460))), Err("scale_mismatch"));
        // no displayed threshold (e.g. Grandmaster): no cross-check
        assert_eq!(validate(&valid(535, None)), Ok(()));
        // overflowing addition: refused, no panic
        assert_eq!(
            validate(&valid(1000, Some(i32::MAX))),
            Err("scale_mismatch")
        );
    }

    #[test]
    fn valid_result_needs_rank_and_total_in_range() {
        let mut b = valid(535, Some(465));
        b.rank_name = None;
        assert_eq!(validate(&b), Err("rank_required"));
        b.rank_name = Some("   ".into());
        assert_eq!(validate(&b), Err("rank_required"));
        assert_eq!(validate(&valid(1001, None)), Err("bad_points"));
        assert_eq!(validate(&valid(-1, None)), Err("bad_points"));
    }

    #[test]
    fn points_delta_is_bounded() {
        let mut b = valid(535, Some(465));
        b.points_delta = Some(486);
        assert_eq!(validate(&b), Ok(()));
        b.points_delta = Some(i32::MIN);
        assert_eq!(validate(&b), Err("bad_points"));
    }

    #[test]
    fn unavailable_has_no_rank_name_either() {
        let mut b = unavailable(Some("titre non lu"));
        b.rank_name = Some("Diamond 1".into());
        assert_eq!(validate(&b), Err("rank_on_unavailable"));
    }

    #[test]
    fn unavailable_has_non_blank_reason_and_no_numbers() {
        assert_eq!(validate(&unavailable(None)), Err("reason_required"));
        assert_eq!(validate(&unavailable(Some(""))), Err("reason_required"));
        assert_eq!(validate(&unavailable(Some("   "))), Err("reason_required"));
        assert_eq!(validate(&unavailable(Some("titre non lu"))), Ok(()));
        let mut b = unavailable(Some("titre non lu"));
        b.points_delta = Some(12);
        assert_eq!(validate(&b), Err("numbers_on_unavailable"));
    }

    #[test]
    fn linked_match_only_when_the_lobby_is_this_session() {
        let me = "c".repeat(64);
        assert_eq!(linked_match(Some((&me, Some(7))), &me), Some(7));
        assert_eq!(linked_match(Some((&me, None)), &me), None);
        assert_eq!(
            linked_match(Some(("d".repeat(64).as_str(), Some(7))), &me),
            None
        );
        assert_eq!(linked_match(None, &me), None);
    }

    #[test]
    fn session_header_must_be_sha256_hex_and_is_lowercased() {
        let mut h = HeaderMap::new();
        assert_eq!(session_from_headers(&h), Err("missing_session"));
        h.insert("x-session", HeaderValue::from_static("xyz"));
        assert_eq!(session_from_headers(&h), Err("bad_session"));
        let upper = "AB".repeat(32);
        h.insert("x-session", HeaderValue::from_str(&upper).unwrap());
        assert_eq!(session_from_headers(&h), Ok("ab".repeat(32)));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod db_tests {
    use super::*;
    use crate::api_tests::test_state;
    use crate::upload::sha256_hex;
    use axum::http::HeaderValue;

    /// Known upload token (hash in the database, clear value in the header); returns the clear value.
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

    fn headers(token: Option<&str>, session: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Some(t) = token {
            h.insert(
                "authorization",
                HeaderValue::from_str(&format!("Bearer {t}")).unwrap(),
            );
        }
        h.insert("x-session", HeaderValue::from_str(session).unwrap());
        h
    }

    fn session() -> String {
        format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        )
    }

    fn body(total: i32, req: i32) -> Bytes {
        Bytes::from(
            json!({ "status": "valide", "rank_name": "Diamond 1", "total_points": total, "promotion_required": req, "points_delta": 486 })
                .to_string(),
        )
    }

    async fn rows_for(db: &sqlx::PgPool, sid: &str) -> Vec<(String, i32, Option<i64>)> {
        sqlx::query_as(
            "SELECT status, total_points, match_id FROM ranked_results WHERE session_id = $1",
        )
        .bind(sid)
        .fetch_all(db)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn post_result_needs_token_even_with_a_bad_body() {
        let Some(state) = test_state(None).await else {
            return;
        };
        let sid = session();
        let (st, _) = post_result(State(state.clone()), headers(None, &sid), body(535, 465)).await;
        assert_eq!(st, StatusCode::UNAUTHORIZED);
        // invalid body without a token: 401, not 422 (the token comes before the body)
        let (st, _) = post_result(
            State(state.clone()),
            headers(None, &sid),
            Bytes::from_static(b"nope"),
        )
        .await;
        assert_eq!(st, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn post_result_stores_reading_and_last_one_wins() {
        let Some(state) = test_state(None).await else {
            return;
        };
        let clear = token(&state.db).await;
        let sid = session();
        let (st, v) = post_result(
            State(state.clone()),
            headers(Some(&clear), &sid),
            body(500, 500),
        )
        .await;
        assert_eq!(st, StatusCode::CREATED, "{v:?}");
        let (st, _) = post_result(
            State(state.clone()),
            headers(Some(&clear), &sid),
            body(535, 465),
        )
        .await;
        assert_eq!(st, StatusCode::CREATED);
        assert_eq!(
            rows_for(&state.db, &sid).await,
            vec![("valide".into(), 535, None)]
        );
    }

    /// Spec rule: an `indisponible` received after a `valide` does not overwrite it (200, `kept`).
    #[tokio::test]
    async fn unavailable_never_overwrites_a_valid_reading() {
        let Some(state) = test_state(None).await else {
            return;
        };
        let clear = token(&state.db).await;
        let sid = session();
        let (st, _) = post_result(
            State(state.clone()),
            headers(Some(&clear), &sid),
            body(535, 465),
        )
        .await;
        assert_eq!(st, StatusCode::CREATED);
        let unavailable =
            Bytes::from_static(br#"{"status":"indisponible","reason":"titre non lu"}"#);
        let (st, v) = post_result(
            State(state.clone()),
            headers(Some(&clear), &sid),
            unavailable,
        )
        .await;
        assert_eq!((st, v.0["kept"].as_bool()), (StatusCode::OK, Some(true)));
        assert_eq!(
            rows_for(&state.db, &sid).await,
            vec![("valide".into(), 535, None)]
        );
    }

    /// Two sends of the same session (e.g. retry after a timeout): a single row.
    #[tokio::test]
    async fn same_session_twice_keeps_one_row() {
        let Some(state) = test_state(None).await else {
            return;
        };
        let clear = token(&state.db).await;
        let sid = session();
        for total in [500, 535] {
            let (st, _) = post_result(
                State(state.clone()),
                headers(Some(&clear), &sid),
                body(total, 1000 - total),
            )
            .await;
            assert_eq!(st, StatusCode::CREATED);
        }
        assert_eq!(
            rows_for(&state.db, &sid).await,
            vec![("valide".into(), 535, None)]
        );
    }

    #[tokio::test]
    async fn post_result_rejects_bad_session_body_and_writes_nothing() {
        let Some(state) = test_state(None).await else {
            return;
        };
        let clear = token(&state.db).await;
        let sid = session();
        let (st, v) = post_result(
            State(state.clone()),
            headers(Some(&clear), &sid),
            body(535, 460),
        )
        .await;
        assert_eq!(
            (st, v.0["error"].as_str()),
            (StatusCode::UNPROCESSABLE_ENTITY, Some("scale_mismatch"))
        );
        let (st, v) = post_result(
            State(state.clone()),
            headers(Some(&clear), &sid),
            Bytes::from_static(b"{"),
        )
        .await;
        assert_eq!(
            (st, v.0["error"].as_str()),
            (StatusCode::UNPROCESSABLE_ENTITY, Some("bad_json"))
        );
        let (st, v) = post_result(
            State(state.clone()),
            headers(Some(&clear), "xyz"),
            body(535, 465),
        )
        .await;
        assert_eq!(
            (st, v.0["error"].as_str()),
            (StatusCode::UNPROCESSABLE_ENTITY, Some("bad_session"))
        );
        assert!(rows_for(&state.db, &sid).await.is_empty());
    }

    #[tokio::test]
    async fn pending_reading_attaches_once_to_its_match() {
        let Some(state) = test_state(None).await else {
            return;
        };
        let clear = token(&state.db).await;
        let sid = session();
        let (st, _) = post_result(
            State(state.clone()),
            headers(Some(&clear), &sid),
            body(535, 465),
        )
        .await;
        assert_eq!(st, StatusCode::CREATED);
        let match_id = crate::players::testutil::insert_match(&state.db, None, &[]).await;
        assert_eq!(attach_to_match(&state.db, &sid, match_id).await.unwrap(), 1);
        assert_eq!(attach_to_match(&state.db, &sid, match_id).await.unwrap(), 0);
        assert_eq!(
            rows_for(&state.db, &sid).await,
            vec![("valide".into(), 535, Some(match_id))]
        );
    }

    /// Re-process: the game is deleted then recreated (new id) with the same fingerprint. The link
    /// must come back through the fingerprint, without going through the lobby.
    #[tokio::test]
    async fn link_survives_a_reprocess_through_the_fingerprint() {
        let Some(state) = test_state(None).await else {
            return;
        };
        let clear = token(&state.db).await;
        let sid = session();
        let (st, _) = post_result(
            State(state.clone()),
            headers(Some(&clear), &sid),
            body(535, 465),
        )
        .await;
        assert_eq!(st, StatusCode::CREATED);
        let first = crate::players::testutil::insert_match(&state.db, None, &[]).await;
        assert_eq!(attach_to_match(&state.db, &sid, first).await.unwrap(), 1);
        let fp: String = sqlx::query_scalar("SELECT fingerprint FROM matches WHERE id = $1")
            .bind(first)
            .fetch_one(&state.db)
            .await
            .unwrap();
        // deletion: ON DELETE SET NULL detaches the reading
        sqlx::query("DELETE FROM matches WHERE id = $1")
            .bind(first)
            .execute(&state.db)
            .await
            .unwrap();
        assert_eq!(
            rows_for(&state.db, &sid).await,
            vec![("valide".into(), 535, None)]
        );
        // recreation with the same fingerprint, then link by fingerprint
        let second = crate::players::testutil::insert_match(&state.db, None, &[]).await;
        sqlx::query("UPDATE matches SET fingerprint = $1 WHERE id = $2")
            .bind(&fp)
            .bind(second)
            .execute(&state.db)
            .await
            .unwrap();
        assert_eq!(
            attach_by_fingerprint(&state.db, &fp, second).await.unwrap(),
            1
        );
        assert_eq!(
            rows_for(&state.db, &sid).await,
            vec![("valide".into(), 535, Some(second))]
        );
    }
}
