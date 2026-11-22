//! REST routes of the draft. Each mutation takes the write-lock, persists, then broadcasts `draft.updated`
//! on the WS (the clients refetch `GET /api/draft`). Write failure → `500 persist_failed` (R24, like
//! cast: the state stays applied in memory and is broadcast). Open mode (local self-hosting), no auth.
use crate::draft::{DraftState, Format, Side, TeamInfo};
use crate::AppState;
use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;
use serde_json::{json, Value as J};
use std::sync::atomic::Ordering;

type Resp = (StatusCode, Json<J>);

/// Persists the current state + broadcasts the real-time event (always, even if the write fails:
/// the in-memory state has changed). The snapshot and its version are taken together under the read
/// lock — no mutation can run in between, concurrent readers copy the same state — so versions follow
/// the order of the states; the single saver task (`store::spawn_saver`) then never lets an older
/// snapshot overwrite a newer one, whatever order concurrent persists reach it in. Write error →
/// `Err` (the routes answer `500 persist_failed`, R24).
pub(crate) async fn persist(s: &AppState) -> Result<(), sqlx::Error> {
    let (snapshot, v) = {
        let d = s.draft.read().await;
        let v = s.draft_version.fetch_add(1, Ordering::SeqCst) + 1;
        (d.clone(), v)
    };
    let res = save_versioned(s, snapshot, v).await;
    if let Err(e) = &res {
        tracing::error!("draft save: {e}");
    }
    let _ = s.events.send(json!({ "type": "draft.updated" }));
    res
}

/// Hands `snapshot` (version `v`) to the saver task and waits for its verdict.
async fn save_versioned(s: &AppState, snapshot: DraftState, v: u64) -> Result<(), sqlx::Error> {
    let gone = || sqlx::Error::Protocol("draft saver task ended".into());
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    if s.draft_save_tx.send((snapshot, v, reply_tx)).await.is_err() {
        // Saver task gone (shutdown): an unpersisted write is a failure (R24), never a silent success.
        return Err(gone());
    }
    reply_rx.await.unwrap_or_else(|_| Err(gone()))
}

/// R24: the database write failed — state applied in memory and broadcast, but not persisted.
pub(crate) fn persist_failed() -> Resp {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": "persist_failed" })),
    )
}

/// Format imposed by the cast series when it is active (`None` otherwise). The cast lock is
/// released before any draft lock (never both at the same time).
pub(crate) async fn series_format(s: &AppState) -> Option<Format> {
    let c = s.cast.read().await;
    c.active.then_some(c.format)
}

fn series_active() -> Resp {
    (
        StatusCode::CONFLICT,
        Json(json!({ "error": "series_active" })),
    )
}

/// GET /api/draft — current state.
pub async fn get_draft(State(s): State<AppState>) -> Json<DraftState> {
    // public view: without replay scripts or the nicknames of unrevealed picks
    Json(s.draft.read().await.public())
}

#[derive(Deserialize)]
pub struct ConfigBody {
    format: Format,
    map: String,
    first_pick: Side,
    #[serde(default)]
    blue: Option<TeamInfo>,
    #[serde(default)]
    red: Option<TeamInfo>,
    #[serde(default)]
    bo: Option<u8>,
}

/// POST /api/draft/config — (re)configures the current game. KEEPS the series context
/// (fearless), the manual pre-bans, the score and the teams: changing the map/format must not
/// erase the pre-bans or the score. Only format/map/first_pick/bo + the game's picks are reset.
///
/// Active cast series: teams, nicknames, Bo (copied from the series) and format belong to the
/// series — the body's `blue`/`red`/`bo` are ignored and `format` forced; map and first_pick stay free.
pub async fn config(
    State(s): State<AppState>,
    Json(b): Json<ConfigBody>,
) -> Result<Json<DraftState>, Resp> {
    let forced = series_format(&s).await;
    let prev = { s.draft.read().await.clone() };
    let mut d = DraftState::new(forced.unwrap_or(b.format), b.first_pick, b.map);
    if forced.is_some() {
        d.blue = prev.blue;
        d.red = prev.red;
        d.bo = prev.bo;
    } else {
        d.blue = b.blue.unwrap_or(prev.blue);
        d.red = b.red.unwrap_or(prev.red);
        d.bo = b.bo.unwrap_or(prev.bo);
    }
    d.score = prev.score;
    d.series_games = prev.series_games;
    d.series_bans = prev.series_bans;
    d.manual_unavailable = prev.manual_unavailable;
    // the replay queue survives a reconfiguration (current game deselected, playback stopped)
    d.replay = super::ReplayState {
        current: None,
        playing: false,
        next_at_ms: None,
        ..prev.replay
    };
    *s.draft.write().await = d;
    persist(&s).await.map_err(|_| persist_failed())?;
    Ok(get_draft(State(s)).await)
}

#[derive(Deserialize)]
pub struct HeroBody {
    hero: String,
}

/// POST /api/draft/action — assigns a hero to the current step.
pub async fn action(State(s): State<AppState>, Json(b): Json<HeroBody>) -> Resp {
    let res = { s.draft.write().await.apply(&b.hero) };
    match res {
        Ok(()) => match persist(&s).await {
            Ok(()) => (StatusCode::OK, Json(json!({ "ok": true }))),
            Err(_) => persist_failed(),
        },
        Err(e) => (
            StatusCode::CONFLICT,
            Json(json!({ "error": format!("{e:?}") })),
        ),
    }
}

/// POST /api/draft/undo
pub async fn undo(State(s): State<AppState>) -> Resp {
    let res = { s.draft.write().await.undo() };
    match res {
        Ok(()) => match persist(&s).await {
            Ok(()) => (StatusCode::OK, Json(json!({ "ok": true }))),
            Err(_) => persist_failed(),
        },
        Err(e) => (
            StatusCode::CONFLICT,
            Json(json!({ "error": format!("{e:?}") })),
        ),
    }
}

/// POST /api/draft/reset
pub async fn reset(State(s): State<AppState>) -> Result<Json<DraftState>, Resp> {
    s.draft.write().await.reset();
    persist(&s).await.map_err(|_| persist_failed())?;
    Ok(get_draft(State(s)).await)
}

#[derive(Deserialize)]
pub struct UnavailableBody {
    hero: String,
    value: bool,
}

/// POST /api/draft/unavailable — manual override of a hero's availability.
pub async fn unavailable(
    State(s): State<AppState>,
    Json(b): Json<UnavailableBody>,
) -> Result<Json<DraftState>, Resp> {
    s.draft.write().await.set_unavailable(&b.hero, b.value);
    persist(&s).await.map_err(|_| persist_failed())?;
    Ok(get_draft(State(s)).await)
}

#[derive(Deserialize)]
pub struct ScoreBody {
    blue: u8,
    red: u8,
}

/// POST /api/draft/score — 409 `series_active` as long as a cast series is active.
pub async fn score(
    State(s): State<AppState>,
    Json(b): Json<ScoreBody>,
) -> Result<Json<DraftState>, Resp> {
    if series_format(&s).await.is_some() {
        return Err(series_active());
    }
    {
        let mut d = s.draft.write().await;
        d.score = [b.blue, b.red];
    }
    persist(&s).await.map_err(|_| persist_failed())?;
    Ok(get_draft(State(s)).await)
}

#[derive(Deserialize)]
pub struct TeamsBody {
    blue: TeamInfo,
    red: TeamInfo,
}

/// POST /api/draft/teams — updates names + nicknames WITHOUT resetting the ongoing draft.
/// 409 `series_active` as long as a cast series is active (the series is the source of truth).
pub async fn teams(
    State(s): State<AppState>,
    Json(b): Json<TeamsBody>,
) -> Result<Json<DraftState>, Resp> {
    if series_format(&s).await.is_some() {
        return Err(series_active());
    }
    {
        let mut d = s.draft.write().await;
        d.blue = b.blue;
        d.red = b.red;
    }
    persist(&s).await.map_err(|_| persist_failed())?;
    Ok(get_draft(State(s)).await)
}

/// POST /api/draft/series/next — closes the current game, accumulates the fearless bans, restarts.
pub async fn series_next(State(s): State<AppState>) -> Result<Json<DraftState>, Resp> {
    s.draft.write().await.start_next_game();
    persist(&s).await.map_err(|_| persist_failed())?;
    Ok(get_draft(State(s)).await)
}

/// POST /api/draft/series/new — new series (clears the history + the series bans).
pub async fn series_new(State(s): State<AppState>) -> Result<Json<DraftState>, Resp> {
    s.draft.write().await.new_series();
    persist(&s).await.map_err(|_| persist_failed())?;
    Ok(get_draft(State(s)).await)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use crate::api_tests::{call, test_state};
    use axum::http::StatusCode;
    use serde_json::json;

    fn team(n: &str) -> serde_json::Value {
        json!({"name": n, "players": ["a", "b", "c", "d", "e"]})
    }

    #[tokio::test]
    async fn draft_teams_route_returns_409_when_series_active() {
        let Some(state) = test_state(None).await else {
            return;
        };
        state.cast.write().await.active = true;
        let before = state.draft.read().await.blue.name.clone();
        for (path, body) in [
            (
                "/api/draft/teams",
                json!({"blue": team("X"), "red": team("Y")}),
            ),
            ("/api/draft/score", json!({"blue": 2, "red": 1})),
        ] {
            let (st, v) = call(&state, "POST", path, Some(body)).await;
            assert_eq!(st, StatusCode::CONFLICT, "{path}");
            assert_eq!(v["error"], "series_active", "{path}");
        }
        let d = state.draft.read().await;
        assert_eq!(d.blue.name, before);
        assert_eq!(d.score, [0, 0]);
    }

    #[tokio::test]
    async fn draft_teams_route_returns_200_when_series_inactive() {
        let Some(state) = test_state(None).await else {
            return;
        };
        assert!(!state.cast.read().await.active);
        let (st, _) = call(
            &state,
            "POST",
            "/api/draft/teams",
            Some(json!({"blue": team("X"), "red": team("Y")})),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        let (st, _) = call(
            &state,
            "POST",
            "/api/draft/score",
            Some(json!({"blue": 2, "red": 1})),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        let d = state.draft.read().await;
        assert_eq!((d.blue.name.as_str(), d.score), ("X", [2, 1]));
    }

    #[tokio::test]
    async fn draft_config_in_active_series_keeps_series_owned_fields() {
        let Some(state) = test_state(None).await else {
            return;
        };
        {
            let mut c = state.cast.write().await;
            c.active = true;
            c.format = crate::draft::Format::Fearless;
        }
        {
            let mut d = state.draft.write().await;
            d.blue.name = "Synced".into();
            d.bo = 7;
        }
        let body = json!({"format": "standard", "map": "Cursed Hollow", "first_pick": "red",
            "blue": team("Hijack"), "red": team("Hijack"), "bo": 1});
        let (st, v) = call(&state, "POST", "/api/draft/config", Some(body)).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(v["format"], "fearless");
        assert_eq!(v["blue"]["name"], "Synced");
        assert_eq!(v["bo"], 7);
        assert_eq!(v["map"], "Cursed Hollow");
        assert_eq!(v["first_pick"], "red");
    }

    #[tokio::test]
    async fn draft_config_inactive_series_behaves_as_before() {
        let Some(state) = test_state(None).await else {
            return;
        };
        let body = json!({"format": "fearless", "map": "Sky Temple", "first_pick": "blue",
            "blue": team("Mine"), "bo": 3});
        let (st, v) = call(&state, "POST", "/api/draft/config", Some(body)).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(
            (
                v["format"].as_str(),
                v["blue"]["name"].as_str(),
                v["bo"].as_u64()
            ),
            (Some("fearless"), Some("Mine"), Some(3))
        );
    }

    /// M6: concurrent mutations must leave the **latest** state in `draft_live`, never the snapshot
    /// of an older mutation that happened to be written last. Runs against a private copy of
    /// `draft_live` (own schema via `search_path`) so the other tests writing the shared singleton
    /// row cannot interfere; many rounds of concurrent writers make the old write race show up.
    #[tokio::test]
    async fn concurrent_draft_mutations_persist_the_latest_state() {
        let Some(base) = test_state(None).await else {
            return;
        };
        let (db, schema) = crate::draft::store::test_support::private_draft_live(&base.db).await;
        let state = crate::test_app_state((*base.cfg).clone(), db);
        let mut stale = 0;
        for round in 0..20u8 {
            let writers: Vec<_> = (0..16u8)
                .map(|i| {
                    let st = state.clone();
                    tokio::spawn(async move {
                        call(
                            &st,
                            "POST",
                            "/api/draft/score",
                            Some(json!({"blue": i, "red": round})),
                        )
                        .await
                        .0
                    })
                })
                .collect();
            for w in writers {
                assert_eq!(w.await.unwrap(), StatusCode::OK);
            }
            let mem = state.draft.read().await.score;
            let saved = crate::draft::store::load(&state.db).await.unwrap().score;
            if saved != mem {
                stale += 1;
            }
        }
        crate::draft::store::test_support::drop_private(&base.db, &schema).await;
        assert_eq!(
            stale, 0,
            "rounds where draft_live kept an older snapshot than memory"
        );
    }

    /// M6 / R24 (same contract as cast): a failed `draft_live` write answers
    /// `500 persist_failed`; the mutation stays applied in memory and `draft.updated` is broadcast.
    #[tokio::test]
    async fn failed_draft_save_answers_500_persist_failed_but_applies_and_broadcasts() {
        let Some(state) = test_state(None).await else {
            return;
        };
        state.db.close().await;
        let mut rx = state.events.subscribe();
        let (st, v) = call(
            &state,
            "POST",
            "/api/draft/score",
            Some(json!({"blue": 2, "red": 1})),
        )
        .await;
        assert_eq!(
            (st, v["error"].as_str()),
            (StatusCode::INTERNAL_SERVER_ERROR, Some("persist_failed"))
        );
        assert_eq!(state.draft.read().await.score, [2, 1]);
        assert_eq!(rx.recv().await.unwrap(), json!({"type": "draft.updated"}));
    }
}
