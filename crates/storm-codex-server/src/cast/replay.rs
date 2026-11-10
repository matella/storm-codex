//! POST /api/cast/games/{n}/replay — drops the Custom replay of a game of the series: decoded on the
//! fly (same path as `draft::replay_api::load`), summary (`summary.rs`) kept in the series
//! state. **Nothing is written to the database** outside the `cast_live` row.
use super::api::{conflict, mutate_ex, DraftSync};
use super::summary::{summary_from_output, SummaryError};
use super::{CastError, GameSummary, Orientation};
use crate::AppState;
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value as J};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Semaphore;

type Resp = (StatusCode, Json<J>);

fn unprocessable(code: &str, message: String) -> Resp {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({ "error": code, "message": message })),
    )
}

/// Temporary file deleted on drop (success, error or decoder panic).
struct TempFile(PathBuf);
impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Decodes the bytes of a `.StormReplay` into a summary, under `parse_sem` like the other parsing routes.
pub(super) async fn decode(s: &AppState, bytes: Bytes) -> Result<GameSummary, Resp> {
    decode_in(s.parse_sem.clone(), std::env::temp_dir(), bytes).await
}

/// Writing the temporary file, decoding and deleting all happen **inside** the `spawn_blocking`
/// (nothing blocking on the executor); the semaphore permit is moved there, and deletion is
/// guaranteed by `TempFile` even if the client disconnects (the blocking task runs to completion).
async fn decode_in(sem: Arc<Semaphore>, dir: PathBuf, bytes: Bytes) -> Result<GameSummary, Resp> {
    let Ok(permit) = sem.acquire_owned().await else {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "pool" })),
        ));
    };
    let parsed = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let tmp = TempFile(dir.join(format!("cast-replay-{}.StormReplay", uuid::Uuid::new_v4())));
        std::fs::write(&tmp.0, &bytes).ok()?;
        Some(storm_stats::process_replay(
            &tmp.0,
            &tmp.0.to_string_lossy(),
        ))
    })
    .await;
    let out = match parsed {
        Ok(Some(out)) => out,
        Ok(None) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "io" })),
            ))
        }
        Err(_) => return Err(unprocessable("parse_failed", "le décodeur a échoué".into())),
    };
    summary_from_output(&out).map_err(|e| {
        let code = match e {
            SummaryError::NotCustom => "not_custom",
            SummaryError::Parse(_) => "parse_failed",
            SummaryError::Missing(_) => "incomplete",
        };
        unprocessable(code, e.to_string())
    })
}

/// Orients the summary on the series (R16, `Series::fill_from_replay`, under the write-lock so on the
/// current series) then fills game `n` (winner, takedowns, duration, summary); persists,
/// syncs the draft, broadcasts `cast.updated`. `409 side_mismatch`: nothing changes.
/// Response: the oriented summary + `orientation` (`evidence` | `assumed` | `forced`) and, if the forced
/// swap contradicts direct orientation evidence, `forced_against_evidence: true`.
/// `series_id` and `map`: identity of the series and map of game `n` taken BEFORE decoding — if another
/// series was started, or if game `n` was removed (R21) then relaunched on another map
/// in the meantime, `409 series_changed` and nothing changes (TOCTOU).
pub(super) async fn apply_to(
    s: &AppState,
    series_id: &str,
    map: &str,
    n: u8,
    summary: GameSummary,
    force_swap: bool,
) -> Resp {
    let res = mutate_ex(s, true, DraftSync::Sync, |c| {
        if c.series_id != series_id || !c.games.iter().any(|g| g.number == n && g.map == map) {
            return Err(CastError::SeriesChanged);
        }
        c.fill_from_replay_oriented(n, summary, force_swap)
    })
    .await;
    match res {
        Ok(((stored, how), _)) => {
            let mut v = serde_json::to_value(&stored).unwrap_or(J::Null);
            if let Some(o) = v.as_object_mut() {
                o.insert("orientation".into(), json!(how.code()));
                if how
                    == (Orientation::Forced {
                        against_evidence: true,
                    })
                {
                    o.insert("forced_against_evidence".into(), json!(true));
                }
            }
            (StatusCode::CREATED, Json(v))
        }
        Err(r) => r,
    }
}

#[derive(Deserialize, Default)]
pub struct ReplayQuery {
    /// `?swap=1`: the operator forces the side swap (replay team 0 = Red).
    #[serde(default)]
    swap: Option<String>,
}

/// POST /api/cast/games/{n}/replay — body = raw bytes of the replay. 201 `GameSummary`;
/// 400 `empty_body`; 409 `SeriesInactive` / `UnknownGame` / `side_mismatch` / `series_changed`; 422 `not_custom` /
/// `parse_failed` / `incomplete`. `?swap=1` forces the side swap.
pub async fn load_game_replay(
    State(s): State<AppState>,
    Path(n): Path<u8>,
    Query(q): Query<ReplayQuery>,
    bytes: Bytes,
) -> Resp {
    let force_swap = matches!(q.swap.as_deref(), Some("1" | "true"));
    if bytes.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "empty_body" })),
        );
    }
    let (series_id, map) = {
        let c = s.cast.read().await;
        if !c.active {
            return conflict(CastError::SeriesInactive);
        }
        let Some(g) = c.games.iter().find(|g| g.number == n) else {
            return conflict(CastError::UnknownGame);
        };
        (c.series_id.clone(), g.map.clone())
    };
    match decode(&s, bytes).await {
        Ok(summary) => apply_to(&s, &series_id, &map, n, summary, force_swap).await,
        Err(r) => r,
    }
}

/// POST /api/cast/replays/inspect — body = raw bytes of a `.StormReplay`. Decodes without writing anything
/// or touching the series: 200 `GameSummary` (map and date for the bulk import). 400 `empty_body`;
/// 422 like the game route.
pub async fn inspect(State(s): State<AppState>, bytes: Bytes) -> Resp {
    if bytes.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "empty_body" })),
        );
    }
    match decode(&s, bytes).await {
        Ok(summary) => (
            StatusCode::OK,
            Json(serde_json::to_value(&summary).unwrap_or(J::Null)),
        ),
        Err(r) => r,
    }
}

#[derive(Deserialize)]
pub struct ImportQuery {
    /// Identity of the series at inspection time: if the series has changed, `409 series_changed`.
    series_id: String,
    /// Pool map to create as a game (`picked`/`decider`, not played yet).
    map: String,
    /// `?swap=1`: same meaning as `ReplayQuery`.
    #[serde(default)]
    swap: Option<String>,
}

/// Creates game `map` then fills it from `summary`, in a single mutation: if the replay is
/// refused, no game is created. The replay's map must be `map` (otherwise `409 map_mismatch`).
pub(super) async fn import_to(
    s: &AppState,
    series_id: &str,
    map: &str,
    summary: GameSummary,
    force_swap: bool,
) -> Resp {
    if summary.map.as_deref() != Some(map) {
        return conflict_code("map_mismatch");
    }
    let res = mutate_ex(s, true, DraftSync::Sync, |c| {
        if c.series_id != series_id {
            return Err(CastError::SeriesChanged);
        }
        let n = c.start_game(map)?;
        let (stored, how) = c.fill_from_replay_oriented(n, summary, force_swap)?;
        Ok((n, stored, how))
    })
    .await;
    match res {
        Ok(((n, stored, how), series)) => {
            let game = series.games.iter().find(|g| g.number == n);
            let mut v = serde_json::to_value(game).unwrap_or(J::Null);
            if let Some(o) = v.as_object_mut() {
                o.insert("orientation".into(), json!(how.code()));
                o.insert(
                    "forced_against_evidence".into(),
                    json!(
                        how == (Orientation::Forced {
                            against_evidence: true
                        })
                    ),
                );
                o.insert(
                    "summary".into(),
                    serde_json::to_value(&stored).unwrap_or(J::Null),
                );
            }
            (StatusCode::CREATED, Json(v))
        }
        Err(r) => r,
    }
}

fn conflict_code(code: &str) -> Resp {
    (StatusCode::CONFLICT, Json(json!({ "error": code })))
}

/// POST /api/cast/replays/import?map=…&swap=… — body = raw bytes. Creates the game of map
/// `map` (next number) and fills it from the replay. 201 = the game (`GameRecord`) +
/// `orientation`, `forced_against_evidence`, `summary`; 409 `map_mismatch` / `SeriesInactive` /
/// `MapNotChosen` / `UnknownMap` / `side_mismatch`…; 400 `empty_body`; 422 like the game route.
pub async fn import_replay(
    State(s): State<AppState>,
    Query(q): Query<ImportQuery>,
    bytes: Bytes,
) -> Resp {
    let force_swap = matches!(q.swap.as_deref(), Some("1" | "true"));
    if bytes.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "empty_body" })),
        );
    }
    match decode(&s, bytes).await {
        Ok(summary) => import_to(&s, &q.series_id, &q.map, summary, force_swap).await,
        Err(r) => r,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::api_tests::{call, call_raw, test_state};
    use crate::cast::TEST_DB_LOCK as DB_LOCK;
    use crate::draft::Side;

    const MAP: &str = "Tomb of the Spider Queen";

    fn aram() -> Vec<u8> {
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../storm-stats/tests/data/silver-city-aram.StormReplay"
        ))
        .unwrap()
    }

    /// Optional real Custom replay (not committed: real nicknames) — `STORM_CAST_CUSTOM_REPLAY`.
    fn real_custom() -> Option<Vec<u8>> {
        let p = std::env::var_os("STORM_CAST_CUSTOM_REPLAY")?;
        std::fs::read(p).ok()
    }

    /// Drop on the current series (identity re-read now).
    async fn apply(state: &AppState, n: u8, summary: GameSummary, force_swap: bool) -> Resp {
        let (id, map) = {
            let c = state.cast.read().await;
            (
                c.series_id.clone(),
                c.games
                    .iter()
                    .find(|g| g.number == n)
                    .map(|g| g.map.clone())
                    .unwrap_or_default(),
            )
        };
        apply_to(state, &id, &map, n, summary, force_swap).await
    }

    /// Item 10: another series started during decoding → 409 `series_changed`, nothing changes.
    #[tokio::test]
    async fn replay_for_a_replaced_series_is_409_series_changed() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else {
            return;
        };
        with_game(&state).await;
        let old_id = state.cast.read().await.series_id.clone();
        assert!(!old_id.is_empty());
        with_game(&state).await; // new series (same map, game 1)
        let before = state.cast.read().await.clone();
        assert_ne!(before.series_id, old_id);
        let (st, v) = apply_to(&state, &old_id, MAP, 1, fixture_summary(), false).await;
        assert_eq!(
            (st, v.0["error"].as_str()),
            (StatusCode::CONFLICT, Some("series_changed"))
        );
        assert_eq!(*state.cast.read().await, before);
        assert_eq!(
            apply(&state, 1, fixture_summary(), false).await.0,
            StatusCode::CREATED
        );
    }

    /// Reuse of the number after R21: game 1 removed then relaunched on another map during
    /// decoding → 409 `series_changed`, state unchanged; the map taken at the pre-check stays applicable.
    #[tokio::test]
    async fn replay_for_a_removed_and_restarted_game_is_409_series_changed() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else {
            return;
        };
        with_game(&state).await;
        let id = state.cast.read().await.series_id.clone();
        assert_eq!(
            call(&state, "DELETE", "/api/cast/games/1", None).await.0,
            StatusCode::OK
        );
        call(
            &state,
            "POST",
            "/api/cast/pool/map",
            Some(json!({"name":"Sky Temple","status":"picked","by":"red"})),
        )
        .await;
        let (_, g) = call(
            &state,
            "POST",
            "/api/cast/games/start",
            Some(json!({"map":"Sky Temple"})),
        )
        .await;
        assert_eq!(g["number"], 1);
        let before = state.cast.read().await.clone();
        let (st, v) = apply_to(&state, &id, MAP, 1, fixture_summary(), false).await;
        assert_eq!(
            (st, v.0["error"].as_str()),
            (StatusCode::CONFLICT, Some("series_changed"))
        );
        assert_eq!(*state.cast.read().await, before);
        assert_eq!(
            apply_to(&state, &id, "Sky Temple", 1, fixture_summary(), false)
                .await
                .0,
            StatusCode::CREATED
        );
    }

    fn fixture_summary() -> GameSummary {
        let fx: J = serde_json::from_str(include_str!("fixtures/custom_summary.json")).unwrap();
        let out = storm_stats::Output {
            hotkey_profiles: Default::default(),
            actions: Default::default(),
            status: 1,
            match_: fx["match"].as_object().cloned(),
            players: fx["players"].as_object().cloned(),
        };
        summary_from_output(&out).unwrap()
    }

    /// Active series + game 1 launched on `MAP`.
    async fn with_game(state: &AppState) {
        let body = json!({"blue":{"name":"Blue Co"},"red":{"name":"Red Co"},"bo":3,"format":"fearless","pool":[MAP, "Sky Temple"]});
        assert_eq!(
            call(state, "POST", "/api/cast/series", Some(body)).await.0,
            StatusCode::OK
        );
        call(
            state,
            "POST",
            "/api/cast/pool/map",
            Some(json!({"name":MAP,"status":"picked","by":"blue"})),
        )
        .await;
        let (_, g) = call(
            state,
            "POST",
            "/api/cast/games/start",
            Some(json!({"map":MAP})),
        )
        .await;
        assert_eq!(g["number"], 1);
    }

    async fn post(state: &AppState, n: u8, bytes: Vec<u8>) -> (StatusCode, J) {
        let (st, b) = call_raw(
            state,
            "POST",
            &format!("/api/cast/games/{n}/replay"),
            Some("application/octet-stream"),
            bytes,
        )
        .await;
        (st, serde_json::from_slice(&b).unwrap_or(J::Null))
    }

    async fn rows(state: &AppState, fp: &str) -> (i64, i64) {
        let m = sqlx::query_scalar("SELECT count(*) FROM matches WHERE fingerprint = $1")
            .bind(fp)
            .fetch_one(&state.db)
            .await
            .unwrap();
        let u = sqlx::query_scalar("SELECT count(*) FROM uploads WHERE fingerprint = $1")
            .bind(fp)
            .fetch_one(&state.db)
            .await
            .unwrap();
        (m, u)
    }

    fn fingerprint(bytes: &[u8]) -> String {
        let dir =
            std::env::temp_dir().join(format!("cast-fp-{}.StormReplay", uuid::Uuid::new_v4()));
        std::fs::write(&dir, bytes).unwrap();
        let out = storm_stats::process_replay(&dir, "x.StormReplay");
        let _ = std::fs::remove_file(&dir);
        crate::upload::game_fingerprint(&out).unwrap()
    }

    /// M2: the temporary file disappears after a successful decoding (ARAM → `not_custom`) as
    /// after a failure (unreadable bytes → `parse_failed`); the permit is returned.
    #[tokio::test]
    async fn decode_removes_temp_file_on_success_and_failure() {
        let dir = std::env::temp_dir().join(format!("cast-decode-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let sem = Arc::new(Semaphore::new(1));
        for (bytes, code) in [(aram(), "not_custom"), (vec![7u8; 64], "parse_failed")] {
            let r = decode_in(sem.clone(), dir.clone(), Bytes::from(bytes)).await;
            assert_eq!(
                r.err().map(|(_, j)| j.0["error"].clone()),
                Some(json!(code))
            );
            assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "{code}");
            assert_eq!(sem.available_permits(), 1);
        }
        // missing folder: io error, no panic
        let missing = dir.join("nope");
        let r = decode_in(sem.clone(), missing, Bytes::from_static(b"x")).await;
        assert_eq!(
            r.err().map(|(st, _)| st),
            Some(StatusCode::INTERNAL_SERVER_ERROR)
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    async fn series_of(state: &AppState) -> String {
        state.cast.read().await.series_id.clone()
    }

    /// A series started between the inspection and the import: 409 `series_changed`, nothing changes.
    #[tokio::test]
    async fn import_for_a_replaced_series_is_409_series_changed() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else {
            return;
        };
        with_pool(&state).await;
        let old = series_of(&state).await;
        with_pool(&state).await; // new series (same map)
        let before = state.cast.read().await.clone();
        let (st, v) = import_to(&state, &old, MAP, fixture_summary(), false).await;
        assert_eq!(
            (st, v.0["error"].as_str()),
            (StatusCode::CONFLICT, Some("series_changed"))
        );
        assert_eq!(*state.cast.read().await, before);
    }

    /// Active series, pool `[MAP, Sky Temple]`, MAP chosen, no game launched.
    async fn with_pool(state: &AppState) {
        let body = json!({"blue":{"name":"Blue Co"},"red":{"name":"Red Co"},"bo":3,"format":"fearless","pool":[MAP, "Sky Temple"]});
        assert_eq!(
            call(state, "POST", "/api/cast/series", Some(body)).await.0,
            StatusCode::OK
        );
        call(
            state,
            "POST",
            "/api/cast/pool/map",
            Some(json!({"name":MAP,"status":"picked","by":"blue"})),
        )
        .await;
    }

    /// Bulk import: the game is created (next number) and filled in one go; without orientation
    /// evidence, team 0 = Blue.
    #[tokio::test]
    async fn import_creates_and_fills_the_game() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else {
            return;
        };
        with_pool(&state).await;
        let (st, v) = import_to(
            &state,
            &series_of(&state).await,
            MAP,
            fixture_summary(),
            false,
        )
        .await;
        assert_eq!(st, StatusCode::CREATED, "{:?}", v.0);
        assert_eq!(
            (
                v.0["number"].as_u64(),
                v.0["winner"].as_str(),
                v.0["orientation"].as_str()
            ),
            (Some(1), Some("blue"), Some("assumed"))
        );
        let c = state.cast.read().await.clone();
        assert_eq!(c.games.len(), 1);
        assert!(c.games[0].summary.is_some());
        assert_eq!(crate::cast::store::load(&state.db).await, Some(c));
    }

    /// Replay map ≠ requested map → 409 `map_mismatch`, nothing changes.
    #[tokio::test]
    async fn import_refuses_a_replay_of_another_map() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else {
            return;
        };
        with_pool(&state).await;
        let before = state.cast.read().await.clone();
        let (st, v) = import_to(
            &state,
            &series_of(&state).await,
            "Sky Temple",
            fixture_summary(),
            false,
        )
        .await;
        assert_eq!(
            (st, v.0["error"].as_str()),
            (StatusCode::CONFLICT, Some("map_mismatch"))
        );
        assert_eq!(*state.cast.read().await, before);
    }

    /// Map already played: refused, and no second game is created.
    #[tokio::test]
    async fn import_of_an_already_played_map_creates_nothing() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else {
            return;
        };
        with_pool(&state).await;
        assert_eq!(
            import_to(
                &state,
                &series_of(&state).await,
                MAP,
                fixture_summary(),
                false
            )
            .await
            .0,
            StatusCode::CREATED
        );
        let before = state.cast.read().await.clone();
        let (st, _) = import_to(
            &state,
            &series_of(&state).await,
            MAP,
            fixture_summary(),
            false,
        )
        .await;
        assert_eq!(st, StatusCode::CONFLICT);
        assert_eq!(*state.cast.read().await, before);
    }

    #[tokio::test]
    async fn inspect_route_writes_nothing_and_rejects_bad_bytes() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else {
            return;
        };
        with_pool(&state).await;
        let before = state.cast.read().await.clone();
        let (st, b) = call_raw(
            &state,
            "POST",
            "/api/cast/replays/inspect",
            Some("application/octet-stream"),
            vec![],
        )
        .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "{}",
            String::from_utf8_lossy(&b)
        );
        let (st, b) = call_raw(
            &state,
            "POST",
            "/api/cast/replays/inspect",
            Some("application/octet-stream"),
            vec![7u8; 64],
        )
        .await;
        assert_eq!(
            st,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{}",
            String::from_utf8_lossy(&b)
        );
        assert_eq!(*state.cast.read().await, before);
    }

    #[tokio::test]
    async fn replay_route_empty_body_is_400() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else {
            return;
        };
        with_game(&state).await;
        let before = state.cast.read().await.clone();
        let (st, v) = post(&state, 1, vec![]).await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
        assert_eq!(v["error"], "empty_body");
        assert_eq!(*state.cast.read().await, before);
    }

    #[tokio::test]
    async fn replay_route_garbage_bytes_is_422_not_500() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else {
            return;
        };
        with_game(&state).await;
        let before = state.cast.read().await.clone();
        let a = aram();
        let half = a[..a.len() / 2].to_vec();
        for (bytes, code) in [
            (vec![7u8; 64], "parse_failed"),
            (half, "parse_failed"),
            (a, "not_custom"),
        ] {
            let (st, v) = post(&state, 1, bytes).await;
            assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
            assert_eq!(v["error"], code);
            assert!(v["message"].is_string());
        }
        assert_eq!(*state.cast.read().await, before);
    }

    #[tokio::test]
    async fn replay_route_needs_active_series_and_known_game() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else {
            return;
        };
        *state.cast.write().await = crate::cast::Series::new();
        let (st, v) = post(&state, 1, vec![7u8; 64]).await;
        assert_eq!(
            (st, v["error"].as_str()),
            (StatusCode::CONFLICT, Some("SeriesInactive"))
        );
        with_game(&state).await;
        let (st, v) = post(&state, 9, vec![7u8; 64]).await;
        assert_eq!(
            (st, v["error"].as_str()),
            (StatusCode::CONFLICT, Some("UnknownGame"))
        );
    }

    #[tokio::test]
    async fn replay_route_writes_nothing_to_matches() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else {
            return;
        };
        with_game(&state).await;
        let mut cases = vec![aram()];
        cases.extend(real_custom());
        for bytes in cases {
            let fp = fingerprint(&bytes);
            let before = rows(&state, &fp).await;
            let (st, _) = post(&state, 1, bytes).await;
            assert!(
                st == StatusCode::CREATED || st == StatusCode::UNPROCESSABLE_ENTITY,
                "{st}"
            );
            assert_eq!(rows(&state, &fp).await, before);
        }
    }

    /// Series teams = fixture nicknames (Player1-5 play in replay team 0).
    async fn name_teams(state: &AppState, blue_first: bool) {
        let five = |o: usize| {
            json!((1..=5)
                .map(|i| json!({"name": format!("Player{}", i + o)}))
                .collect::<Vec<_>>())
        };
        let (b, r) = if blue_first {
            (five(0), five(5))
        } else {
            (five(5), five(0))
        };
        let body = json!({"blue": {"name": "Blue Co", "players": b}, "red": {"name": "Red Co", "players": r}});
        assert_eq!(
            call(state, "PUT", "/api/cast/teams", Some(body)).await.0,
            StatusCode::OK
        );
    }

    /// Fill path (always run): summary of the real fixture oriented by the nicknames →
    /// `GameRecord`, score, draft sync, broadcast. With `STORM_CAST_CUSTOM_REPLAY`, the same
    /// check also goes through the HTTP route with the real bytes (including `?swap=1`).
    #[tokio::test]
    async fn replay_route_fills_game_record() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else {
            return;
        };
        with_game(&state).await;
        // the series' Blue played in replay team 1: evidence by the nicknames → swap
        name_teams(&state, false).await;
        let mut rx = state.events.subscribe();
        let (st, v) = apply(&state, 1, fixture_summary(), false).await;
        assert_eq!(st, StatusCode::CREATED);
        assert_eq!(v.0["takedowns"], json!([15, 28]));
        assert_eq!(v.0["winner"], "red");
        assert_eq!(v.0["orientation"], "evidence");
        assert!(v.0.get("forced_against_evidence").is_none());
        let c = state.cast.read().await.clone();
        let g = &c.games[0];
        assert_eq!(
            (g.winner, g.takedowns, g.duration_secs),
            (Some(Side::Red), [15, 28], Some(1576))
        );
        let s = g.summary.as_ref().unwrap();
        assert_eq!(
            (s.players[0].side, s.players[0].name.as_str()),
            (Side::Blue, "Player6")
        );
        assert_eq!(s.xp_lead[0], (60, -710));
        assert_eq!(state.draft.read().await.score, [0, 1]);
        assert_eq!(crate::cast::store::load(&state.db).await, Some(c));
        let mut seen = vec![];
        while let Ok(e) = rx.try_recv() {
            seen.push(e["type"].as_str().unwrap_or("").to_string());
        }
        assert!(seen.contains(&"cast.updated".to_string()), "{seen:?}");
        // unknown game: 409, nothing changes
        let before = state.cast.read().await.clone();
        assert_eq!(
            apply(&state, 7, fixture_summary(), false).await.0,
            StatusCode::CONFLICT
        );
        assert_eq!(*state.cast.read().await, before);
    }

    /// R16: without orientation evidence, an entered winner that contradicts the replay → 409
    /// `side_mismatch`, state intact; `swap` (operator choice) inverts winner, sides and XP.
    #[tokio::test]
    async fn replay_route_side_mismatch_and_forced_swap() {
        let _g = DB_LOCK.lock().await;
        let Some(state) = test_state(None).await else {
            return;
        };
        with_game(&state).await;
        call(
            &state,
            "POST",
            "/api/cast/games/1/winner",
            Some(json!({"side":"red"})),
        )
        .await;
        let before = state.cast.read().await.clone();
        let (st, v) = apply(&state, 1, fixture_summary(), false).await;
        assert_eq!(
            (st, v.0["error"].as_str()),
            (StatusCode::CONFLICT, Some("side_mismatch"))
        );
        assert_eq!(*state.cast.read().await, before);
        let (st, v) = apply(&state, 1, fixture_summary(), true).await;
        assert_eq!(st, StatusCode::CREATED);
        assert_eq!(
            (v.0["winner"].as_str(), v.0["xp_lead"][0][1].as_i64()),
            (Some("red"), Some(-710))
        );
        assert_eq!(v.0["players"][0]["side"], "blue");
        assert_eq!(v.0["players"][0]["name"], "Player6");
        assert_eq!(
            (
                v.0["orientation"].as_str(),
                v.0.get("forced_against_evidence")
            ),
            (Some("forced"), None)
        );

        // ?swap=1 against direct orientation evidence: accepted but flagged
        with_game(&state).await;
        name_teams(&state, true).await;
        let (st, v) = apply(&state, 1, fixture_summary(), true).await;
        assert_eq!(st, StatusCode::CREATED);
        assert_eq!(
            (
                v.0["orientation"].as_str(),
                v.0["forced_against_evidence"].as_bool()
            ),
            (Some("forced"), Some(true))
        );
        assert_eq!(v.0["winner"], "red");

        if let Some(bytes) = real_custom() {
            // HTTP, real bytes: same refusal without evidence, then ?swap=1
            with_game(&state).await;
            call(
                &state,
                "POST",
                "/api/cast/games/1/winner",
                Some(json!({"side":"red"})),
            )
            .await;
            let before = state.cast.read().await.clone();
            let (st, v) = post(&state, 1, bytes.clone()).await;
            assert_eq!(
                (st, v["error"].as_str()),
                (StatusCode::CONFLICT, Some("side_mismatch")),
                "{v}"
            );
            assert_eq!(*state.cast.read().await, before);
            let t0 = std::time::Instant::now();
            let (st, b) = call_raw(
                &state,
                "POST",
                "/api/cast/games/1/replay?swap=1",
                Some("application/octet-stream"),
                bytes,
            )
            .await;
            eprintln!("cast replay route (real Custom bytes): {:?}", t0.elapsed());
            assert_eq!(st, StatusCode::CREATED);
            let v: J = serde_json::from_slice(&b).unwrap();
            let g = state.cast.read().await.games[0].clone();
            let s = g.summary.unwrap();
            assert_eq!(g.winner, Some(s.winner));
            assert_eq!(v["winner"], "red"); // Tomb 2026-09-27: team 0 wins → swapped = red
            assert_eq!(s.players.len(), 10);
            assert!(s.xp_lead.last().unwrap().1 < 0);
        }
    }
}
