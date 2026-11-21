//! Daily goal tracker (`docs/specs/2026-11-20-streamer-widgets.md`, widget 7): a single operator-set
//! goal (table `app_goal`, one row) and its live progress for the day. `GET`/`PUT /api/admin/goal`
//! (admin-gated, same contract as `manage::put_settings`) set/clear the goal; `GET /api/goal/progress`
//! (public, read-only like the other OBS endpoints) recomputes progress from the archive on every
//! read — nothing but the goal definition itself is persisted.
//!
//! The pure pieces ([`GoalKind`], [`day_bounds`], [`compute_progress`]) have no I/O and are covered by
//! native unit tests (midnight reset, kind switch, no-goal state); the handlers below are the thin SQL
//! layer on top, covered by the endpoint tests in `api_tests`.

use crate::AppState;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use chrono::{DateTime, FixedOffset, Local, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as J;
use thiserror::Error;

fn is_admin(headers: &HeaderMap, state: &AppState) -> bool {
    crate::auth::is_admin(headers, state.cfg.admin_token.as_deref())
}

fn forbidden() -> (StatusCode, Json<J>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({"error": "admin token requis"})),
    )
}

fn db_err(e: sqlx::Error) -> (StatusCode, Json<J>) {
    tracing::error!("goal: {e}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": "db"})),
    )
}

// ── Pure domain ──────────────────────────────────────────────────────────────

/// The two goal kinds the operator can pick (`docs/specs/2026-11-20-streamer-widgets.md`): wins today,
/// or games played today — both counted from the operator's own perspective rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalKind {
    WinsToday,
    GamesToday,
}

#[derive(Debug, Error)]
#[error("unknown goal kind {0:?} (expected wins_today or games_today)")]
pub struct UnknownKind(String);

impl GoalKind {
    pub fn as_str(self) -> &'static str {
        match self {
            GoalKind::WinsToday => "wins_today",
            GoalKind::GamesToday => "games_today",
        }
    }

    pub fn parse(s: &str) -> Result<Self, UnknownKind> {
        match s {
            "wins_today" => Ok(GoalKind::WinsToday),
            "games_today" => Ok(GoalKind::GamesToday),
            other => Err(UnknownKind(other.to_string())),
        }
    }
}

/// `GET /api/goal/progress` response. `kind`/`target`/`reset_at` are all `None` together when no goal
/// is set — the OBS source renders a blank frame in that case.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Progress {
    pub kind: Option<&'static str>,
    pub target: Option<i32>,
    pub done: i64,
    pub reset_at: Option<String>,
}

/// The local-day window (as UTC instants) containing `now`, in the given offset. `now`'s offset is
/// taken as the operator's local time, so the goal resets at **local** midnight, not UTC midnight;
/// injectable so the reset boundary is testable without depending on the real clock.
pub fn day_bounds(now: DateTime<FixedOffset>) -> (DateTime<Utc>, DateTime<Utc>) {
    let date = now.date_naive();
    // midnight (0, 0, 0) is always a valid time of day; the fallback only guards the type, it is
    // never actually reached.
    let start_naive = date
        .and_hms_opt(0, 0, 0)
        .unwrap_or_else(|| now.naive_local());
    let start_local = start_naive
        .and_local_timezone(*now.offset())
        .single()
        .unwrap_or(now);
    let end_local = start_local + chrono::Duration::days(1);
    (
        start_local.with_timezone(&Utc),
        end_local.with_timezone(&Utc),
    )
}

/// Progress for the current goal (if any), given the day's tallies already queried from the archive.
/// Pure: the kind switch (which tally counts as `done`) and the no-goal state are both decided here.
pub fn compute_progress(
    goal: Option<(GoalKind, i32)>,
    wins_today: i64,
    games_today: i64,
    reset_at: DateTime<Utc>,
) -> Progress {
    match goal {
        None => Progress {
            kind: None,
            target: None,
            done: 0,
            reset_at: None,
        },
        Some((kind, target)) => Progress {
            kind: Some(kind.as_str()),
            target: Some(target),
            done: match kind {
                GoalKind::WinsToday => wins_today,
                GoalKind::GamesToday => games_today,
            },
            reset_at: Some(reset_at.to_rfc3339()),
        },
    }
}

/// The operator's local offset (box timezone). A fixed offset so the pure day-boundary math stays
/// identical in production and in tests.
fn local_offset() -> FixedOffset {
    *Local::now().offset()
}

/// `now`, carrying the operator's local offset.
fn now_local() -> DateTime<FixedOffset> {
    Utc::now().with_timezone(&local_offset())
}

// ── Handlers ─────────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct GoalRow {
    kind: String,
    target: i32,
}

async fn load_goal(db: &sqlx::PgPool) -> Result<Option<GoalRow>, sqlx::Error> {
    sqlx::query_as::<_, GoalRow>("SELECT kind, target FROM app_goal WHERE id = 1")
        .fetch_optional(db)
        .await
}

/// `GET /api/admin/goal` (admin) — the current goal, or `{"kind":null,"target":null}` when none is set.
pub async fn get_goal(State(s): State<AppState>, headers: HeaderMap) -> (StatusCode, Json<J>) {
    if !is_admin(&headers, &s) {
        return forbidden();
    }
    match load_goal(&s.db).await {
        Ok(row) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "kind": row.as_ref().map(|r| r.kind.clone()),
                "target": row.as_ref().map(|r| r.target),
            })),
        ),
        Err(e) => db_err(e),
    }
}

#[derive(Deserialize)]
pub struct GoalPatch {
    /// `None`/omitted clears the goal (idempotent — clearing an already-cleared goal is `200 OK`).
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    target: Option<i32>,
}

/// `PUT /api/admin/goal` (admin) — sets the goal (`{"kind":"wins_today","target":3}`) or clears it
/// (`{"kind":null}` or an empty body). A `target` that is missing or not a positive integer when
/// `kind` is given is a `400` — the archive never stores a target that can never complete.
pub async fn put_goal(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(b): Json<GoalPatch>,
) -> (StatusCode, Json<J>) {
    if !is_admin(&headers, &s) {
        return forbidden();
    }
    let Some(kind_str) = b.kind.as_deref().filter(|k| !k.is_empty()) else {
        return match sqlx::query("DELETE FROM app_goal WHERE id = 1")
            .execute(&s.db)
            .await
        {
            Ok(_) => (
                StatusCode::OK,
                Json(serde_json::json!({"kind": null, "target": null})),
            ),
            Err(e) => db_err(e),
        };
    };
    let Ok(kind) = GoalKind::parse(kind_str) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "kind must be wins_today or games_today"})),
        );
    };
    let Some(target) = b.target.filter(|t| *t > 0) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "target must be a positive integer"})),
        );
    };
    match sqlx::query(
        "INSERT INTO app_goal (id, kind, target) VALUES (1, $1, $2)
         ON CONFLICT (id) DO UPDATE SET kind = EXCLUDED.kind, target = EXCLUDED.target, created_at = now()",
    )
    .bind(kind.as_str())
    .bind(target)
    .execute(&s.db)
    .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({"kind": kind.as_str(), "target": target})),
        ),
        Err(e) => db_err(e),
    }
}

/// `GET /api/goal/progress` (public, no auth) — recomputed from the archive on every read, like the
/// other OBS endpoints; nothing here is stored.
pub async fn progress(State(s): State<AppState>) -> (StatusCode, Json<Progress>) {
    let goal = match load_goal(&s.db).await {
        Ok(row) => row,
        Err(e) => {
            tracing::error!("goal progress: load_goal: {e}");
            None
        }
    };
    let Some(row) = goal else {
        let p = compute_progress(None, 0, 0, Utc::now());
        return (StatusCode::OK, Json(p));
    };
    // `GoalKind::parse` cannot fail here: the row only ever holds a value the CHECK constraint (and
    // `put_goal`) already restricted to the two known kinds.
    let Ok(kind) = GoalKind::parse(&row.kind) else {
        tracing::error!("goal progress: unexpected kind {:?} in app_goal", row.kind);
        return (
            StatusCode::OK,
            Json(compute_progress(None, 0, 0, Utc::now())),
        );
    };
    let (start, end) = day_bounds(now_local());
    let tallies = sqlx::query_as::<_, (i64, i64)>(
        "WITH ops AS (
            SELECT lower(jsonb_array_elements_text(value)) AS name
            FROM app_settings WHERE key = 'operator_names'
         )
         SELECT count(*) FILTER (WHERE mp.win), count(*)
         FROM matches m
         JOIN match_players mp ON mp.match_id = m.id
         WHERE m.played_at >= $1 AND m.played_at < $2
           AND (lower(mp.name) IN (SELECT name FROM ops) OR mp.toon_handle = ANY($3::text[]))",
    )
    .bind(start)
    .bind(end)
    .bind(crate::standalone::operator_toons(&s).await)
    .fetch_one(&s.db)
    .await;
    let (wins_today, games_today) = match tallies {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("goal progress: tally query: {e}");
            (0, 0)
        }
    };
    let p = compute_progress(Some((kind, row.target)), wins_today, games_today, end);
    (StatusCode::OK, Json(p))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn fo(hours: i32) -> FixedOffset {
        FixedOffset::east_opt(hours * 3600).expect("valid offset")
    }

    // ── midnight reset ───────────────────────────────────────────────────────

    #[test]
    fn day_bounds_resets_at_local_midnight_not_utc() {
        // 23:30 in UTC+9 (Tokyo-ish): the local day still has 30 minutes left, and the reset instant
        // (next local midnight) is NOT the same as the next UTC midnight.
        let offset = fo(9);
        let now = offset
            .with_ymd_and_hms(2026, 11, 20, 23, 30, 0)
            .single()
            .unwrap();
        let (start, end) = day_bounds(now);
        assert_eq!(start.to_rfc3339(), "2026-11-19T15:00:00+00:00");
        assert_eq!(end.to_rfc3339(), "2026-11-20T15:00:00+00:00");
        assert!(now.with_timezone(&Utc) >= start && now.with_timezone(&Utc) < end);
    }

    #[test]
    fn day_bounds_handles_negative_offset_and_just_after_midnight() {
        // 00:01 local in UTC-5: just ticked over, the window should start a minute earlier in UTC.
        let offset = fo(-5);
        let now = offset
            .with_ymd_and_hms(2026, 11, 20, 0, 1, 0)
            .single()
            .unwrap();
        let (start, end) = day_bounds(now);
        assert_eq!(start.to_rfc3339(), "2026-11-20T05:00:00+00:00");
        assert_eq!(end.to_rfc3339(), "2026-11-21T05:00:00+00:00");
    }

    #[test]
    fn progress_reset_at_matches_day_bounds_end() {
        let offset = fo(2);
        let now = offset
            .with_ymd_and_hms(2026, 11, 20, 12, 0, 0)
            .single()
            .unwrap();
        let (_, end) = day_bounds(now);
        let p = compute_progress(Some((GoalKind::WinsToday, 3)), 1, 2, end);
        assert_eq!(p.reset_at, Some(end.to_rfc3339()));
    }

    // ── kind switch ──────────────────────────────────────────────────────────

    #[test]
    fn wins_today_counts_wins_not_games() {
        let p = compute_progress(Some((GoalKind::WinsToday, 3)), 2, 5, Utc::now());
        assert_eq!(p.kind, Some("wins_today"));
        assert_eq!(p.target, Some(3));
        assert_eq!(p.done, 2);
    }

    #[test]
    fn games_today_counts_games_not_wins() {
        let p = compute_progress(Some((GoalKind::GamesToday, 3)), 2, 5, Utc::now());
        assert_eq!(p.kind, Some("games_today"));
        assert_eq!(p.done, 5);
    }

    #[test]
    fn switching_kind_with_same_tallies_changes_done() {
        let wins_view = compute_progress(Some((GoalKind::WinsToday, 10)), 4, 9, Utc::now());
        let games_view = compute_progress(Some((GoalKind::GamesToday, 10)), 4, 9, Utc::now());
        assert_ne!(wins_view.done, games_view.done);
        assert_eq!(wins_view.done, 4);
        assert_eq!(games_view.done, 9);
    }

    // ── no-goal state ────────────────────────────────────────────────────────

    #[test]
    fn no_goal_gives_null_kind_target_and_reset() {
        let p = compute_progress(None, 7, 7, Utc::now());
        assert_eq!(p.kind, None);
        assert_eq!(p.target, None);
        assert_eq!(p.done, 0);
        assert_eq!(p.reset_at, None);
    }

    #[test]
    fn parse_rejects_unknown_kind() {
        assert!(GoalKind::parse("games_this_week").is_err());
        assert!(GoalKind::parse("").is_err());
    }

    #[test]
    fn parse_round_trips_known_kinds() {
        assert_eq!(GoalKind::parse("wins_today").unwrap(), GoalKind::WinsToday);
        assert_eq!(
            GoalKind::parse(GoalKind::GamesToday.as_str()).unwrap(),
            GoalKind::GamesToday
        );
    }

    #[test]
    fn local_offset_and_now_local_are_valid_production_entry_points() {
        // smoke-tests the production entry points without depending on the host's real timezone.
        let _ = local_offset();
        let _ = now_local();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod endpoint_tests {
    use super::*;
    use crate::api_tests::test_state;
    use crate::players::testutil::{insert_match, row, uid};
    use axum::http::HeaderValue;

    fn bearer(token: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
        h
    }

    #[tokio::test]
    async fn put_and_get_goal_require_admin_token_when_configured() {
        let Some(state) = test_state(Some("sekret")).await else {
            return;
        };
        let body = GoalPatch {
            kind: Some("wins_today".into()),
            target: Some(3),
        };
        // no token at all
        let (st, _) = put_goal(State(state.clone()), HeaderMap::new(), Json(body)).await;
        assert_eq!(st, StatusCode::UNAUTHORIZED);
        // wrong token
        let body = GoalPatch {
            kind: Some("wins_today".into()),
            target: Some(3),
        };
        let (st, _) = put_goal(State(state.clone()), bearer("faux"), Json(body)).await;
        assert_eq!(st, StatusCode::UNAUTHORIZED);
        let (st, _) = get_goal(State(state.clone()), HeaderMap::new()).await;
        assert_eq!(st, StatusCode::UNAUTHORIZED);
    }

    // `app_goal` is a true singleton (one row, `id=1`), unlike the uuid-scoped fixtures elsewhere in
    // this test suite: concurrent `#[tokio::test]`s mutating it would race each other. The read-only
    // gating test above is safe (it only asserts status codes), but every test that writes or reads
    // back the row's *content* lives in this single sequential test.
    #[tokio::test]
    async fn put_goal_lifecycle_and_progress_from_operator_perspective() {
        let Some(state) = test_state(Some("sekret")).await else {
            return;
        };

        // validation: rejected before any write, so these are safe to run first
        let bad_kind = GoalPatch {
            kind: Some("games_this_week".into()),
            target: Some(3),
        };
        let (st, _) = put_goal(State(state.clone()), bearer("sekret"), Json(bad_kind)).await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
        let bad_target = GoalPatch {
            kind: Some("wins_today".into()),
            target: Some(0),
        };
        let (st, _) = put_goal(State(state.clone()), bearer("sekret"), Json(bad_target)).await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
        let missing_target = GoalPatch {
            kind: Some("wins_today".into()),
            target: None,
        };
        let (st, _) = put_goal(State(state.clone()), bearer("sekret"), Json(missing_target)).await;
        assert_eq!(st, StatusCode::BAD_REQUEST);

        // set, then get reads it back
        let first = GoalPatch {
            kind: Some("games_today".into()),
            target: Some(5),
        };
        let (st, v) = put_goal(State(state.clone()), bearer("sekret"), Json(first)).await;
        assert_eq!(st, StatusCode::OK, "{v:?}");
        let (st, v) = get_goal(State(state.clone()), bearer("sekret")).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(v.0["kind"], "games_today");
        assert_eq!(v.0["target"], 5);

        // a second PUT upserts the same row (never a second row)
        let second = GoalPatch {
            kind: Some("wins_today".into()),
            target: Some(9),
        };
        let _ = put_goal(State(state.clone()), bearer("sekret"), Json(second)).await;
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM app_goal")
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(
            n, 1,
            "a second PUT replaces the single row, it does not add one"
        );
        let (_, v) = get_goal(State(state.clone()), bearer("sekret")).await;
        assert_eq!(v.0["kind"], "wins_today");

        // progress for a real operator, counting only today's games
        let toon = format!("t-goal-{}", uid());
        let name = format!("Goalie{}", uid());
        // append to the shared `operator_names` row without dropping other tests' names (same
        // pattern as `insights::api::session_endpoints_list_review_and_navigate`)
        sqlx::query(
            "INSERT INTO app_settings (key, value) VALUES ('operator_names', jsonb_build_array($1::text))
             ON CONFLICT (key) DO UPDATE SET value = app_settings.value || EXCLUDED.value",
        )
        .bind(&name)
        .execute(&state.db)
        .await
        .unwrap();
        let now = Utc::now();
        // two games today: one win, one loss
        insert_match(&state.db, Some(now), &[row(&toon, &name, 0, "Muradin")]).await;
        insert_match(&state.db, Some(now), &[row(&toon, &name, 1, "Muradin")]).await;
        // a game from yesterday must not count
        insert_match(
            &state.db,
            Some(now - chrono::Duration::days(1)),
            &[row(&toon, &name, 0, "Muradin")],
        )
        .await;
        // the goal from above is still `wins_today`; the operator's own matches above are not tied
        // to `operator_names` (different random name per run), so re-point the goal with that name
        let set_wins = GoalPatch {
            kind: Some("wins_today".into()),
            target: Some(5),
        };
        let _ = put_goal(State(state.clone()), bearer("sekret"), Json(set_wins)).await;
        let (_, v) = progress(State(state.clone())).await;
        assert_eq!(v.0.kind, Some("wins_today"));
        assert_eq!(v.0.target, Some(5));
        assert_eq!(v.0.done, 1, "only today's win counts");
        assert!(v.0.reset_at.is_some());

        let set_games = GoalPatch {
            kind: Some("games_today".into()),
            target: Some(5),
        };
        let _ = put_goal(State(state.clone()), bearer("sekret"), Json(set_games)).await;
        let (_, v) = progress(State(state.clone())).await;
        assert_eq!(v.0.kind, Some("games_today"));
        assert_eq!(
            v.0.done, 2,
            "both of today's games count, win and loss alike"
        );

        // clear: omitted kind
        let clear = GoalPatch {
            kind: None,
            target: None,
        };
        let (st, v) = put_goal(State(state.clone()), bearer("sekret"), Json(clear)).await;
        assert_eq!(st, StatusCode::OK, "{v:?}");
        let (_, v) = get_goal(State(state.clone()), bearer("sekret")).await;
        assert!(v.0["kind"].is_null());
        assert!(v.0["target"].is_null());
        let (st, v) = progress(State(state.clone())).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(v.0.kind, None);
        assert_eq!(v.0.target, None);
        assert_eq!(v.0.done, 0);
        assert_eq!(v.0.reset_at, None);

        // clearing an already-cleared goal stays OK (idempotent)
        let clear_again = GoalPatch {
            kind: None,
            target: None,
        };
        let (st, _) = put_goal(State(state.clone()), bearer("sekret"), Json(clear_again)).await;
        assert_eq!(st, StatusCode::OK);
    }
}
