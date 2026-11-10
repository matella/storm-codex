//! REST routes of the draft from a replay + auto playback timer. Each mutation takes the
//! write-lock only once (so a manual "Next" and a tick never collide),
//! persists, then broadcasts `draft.updated`. Open mode like the rest of `/api/draft`.
use super::api::{persist, series_format};
use super::replay_extract::{loaded_game_from_output, ReplayError};
use super::{DraftError, ReplayState};
use crate::AppState;
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value as J};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

type Resp = (StatusCode, Json<J>);

/// Bounds of the auto playback interval (ms): at least 10 ms (tests), at most 1 h.
const INTERVAL_RANGE: std::ops::RangeInclusive<u32> = 10..=3_600_000;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

fn conflict(e: DraftError) -> Resp {
    (
        StatusCode::CONFLICT,
        Json(json!({ "error": format!("{e:?}") })),
    )
}

fn ok_state(r: &ReplayState) -> Resp {
    (
        StatusCode::OK,
        Json(serde_json::to_value(r).unwrap_or(J::Null)),
    )
}

/// Stops the auto playback timer (if it runs).
fn cancel_timer(s: &AppState) {
    if let Some(h) = s.replay_timer.lock().ok().and_then(|mut g| g.take()) {
        h.abort();
    }
}

/// (Re)starts the timer: a tick reveals a phase every `interval_ms`, until the end.
fn start_timer(s: &AppState) {
    cancel_timer(s);
    let st = s.clone();
    let handle = tokio::spawn(async move {
        loop {
            let ms = st.draft.read().await.replay.interval_ms;
            tokio::time::sleep(Duration::from_millis(u64::from(ms))).await;
            let stop = {
                let mut d = st.draft.write().await;
                if !d.replay.playing {
                    return;
                }
                if d.reveal_phase().is_err() || d.is_complete() {
                    d.replay.playing = false;
                }
                d.replay.next_at_ms = d.replay.playing.then(|| now_ms() + i64::from(ms));
                !d.replay.playing
            };
            persist(&st).await;
            if stop {
                return;
            }
        }
    });
    if let Ok(mut g) = s.replay_timer.lock() {
        *g = Some(handle);
    }
}

/// GET /api/draft/replay — complete queue **with scripts** (reserved for the remote control page).
pub async fn get_replay(State(s): State<AppState>) -> Json<ReplayState> {
    Json(s.draft.read().await.replay.clone())
}

/// Temporary file deleted on drop (success, error or decoder panic).
struct TempFile(PathBuf);
impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Decodes the bytes of a `.StormReplay` under `parse_sem`. Writing the temporary file, decoding
/// and deleting all happen **inside** the `spawn_blocking` (nothing blocking on the executor); the permit
/// is moved there and `TempFile` guarantees deletion, even if the client disconnects (the blocking
/// task runs to completion). Same path as `cast::replay::decode_in`.
async fn decode_in(
    sem: Arc<Semaphore>,
    dir: PathBuf,
    bytes: Bytes,
) -> Result<storm_stats::Output, Resp> {
    let Ok(permit) = sem.acquire_owned().await else {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "pool" })),
        ));
    };
    let parsed = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let tmp = TempFile(dir.join(format!("draft-replay-{}.StormReplay", uuid::Uuid::new_v4())));
        std::fs::write(&tmp.0, &bytes).ok()?;
        Some(storm_stats::process_replay(
            &tmp.0,
            &tmp.0.to_string_lossy(),
        ))
    })
    .await;
    match parsed {
        Ok(Some(out)) => Ok(out),
        Ok(None) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "io" })),
        )),
        Err(_) => Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": "parse_failed" })),
        )),
    }
}

/// POST /api/draft/replay/load — body = bytes of a `.StormReplay` (optional `X-Filename` header).
/// Decodes only the draft: nothing is written to the database. Custom only.
pub async fn load(State(s): State<AppState>, headers: HeaderMap, bytes: Bytes) -> Resp {
    if bytes.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "empty_body" })),
        );
    }
    let name = crate::upload::filename(&headers).unwrap_or_else(|| "replay.StormReplay".into());
    let out = match decode_in(s.parse_sem.clone(), std::env::temp_dir(), bytes).await {
        Ok(out) => out,
        Err(r) => return r,
    };
    match loaded_game_from_output(&out, &name, storm_stats::constants::hero_attribute()) {
        Ok(g) => {
            let over = series_format(&s).await;
            let r = {
                let mut d = s.draft.write().await;
                d.add_game_with(g, over);
                d.public().replay
            };
            persist(&s).await;
            (
                StatusCode::CREATED,
                Json(serde_json::to_value(r).unwrap_or(J::Null)),
            )
        }
        Err(e) => {
            let code = match e {
                ReplayError::NotCustom => "not_custom",
                ReplayError::NoDraft => "no_draft",
                ReplayError::Parse(_) => "parse_failed",
            };
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({ "error": code, "message": e.to_string() })),
            )
        }
    }
}

/// Applies a mutation under a single write-lock, persists, answers with the public queue.
async fn mutate(
    s: &AppState,
    f: impl FnOnce(&mut super::DraftState) -> Result<(), DraftError>,
) -> Resp {
    let res = {
        let mut d = s.draft.write().await;
        f(&mut d).map(|()| d.public().replay)
    };
    match res {
        Ok(r) => {
            persist(s).await;
            ok_state(&r)
        }
        Err(e) => conflict(e),
    }
}

/// After a manual action during auto playback, the next tick restarts from a full interval.
async fn rearm_if_playing(s: &AppState) {
    let playing = {
        let mut d = s.draft.write().await;
        if d.replay.playing {
            let ms = d.replay.interval_ms;
            d.replay.next_at_ms = Some(now_ms() + i64::from(ms));
        }
        d.replay.playing
    };
    if playing {
        start_timer(s);
    }
}

/// POST /api/draft/replay/next — reveals the next phase.
pub async fn next(State(s): State<AppState>) -> Resp {
    let r = mutate(&s, |d| {
        d.reveal_phase()?;
        if d.is_complete() {
            d.replay.playing = false;
            d.replay.next_at_ms = None;
        }
        Ok(())
    })
    .await;
    if r.0 == StatusCode::OK {
        rearm_if_playing(&s).await;
    }
    r
}

/// POST /api/draft/replay/back — undoes the last revealed phase.
pub async fn back(State(s): State<AppState>) -> Resp {
    let r = mutate(&s, |d| d.back_phase()).await;
    if r.0 == StatusCode::OK {
        rearm_if_playing(&s).await;
    }
    r
}

/// POST /api/draft/replay/reset — goes back to step 0 of the current game (paused).
pub async fn reset(State(s): State<AppState>) -> Resp {
    cancel_timer(&s);
    mutate(&s, |d| {
        d.replay.current.ok_or(DraftError::NoGame)?;
        d.reset();
        d.replay.playing = false;
        d.replay.next_at_ms = None;
        Ok(())
    })
    .await
}

#[derive(Deserialize)]
pub struct IndexBody {
    index: usize,
}

/// POST /api/draft/replay/select — prepares game `index` of the queue; an active cast series
/// imposes its format (M5, `select_game_with`).
pub async fn select(State(s): State<AppState>, Json(b): Json<IndexBody>) -> Resp {
    cancel_timer(&s);
    let over = series_format(&s).await;
    mutate(&s, |d| d.select_game_with(b.index, over)).await
}

/// DELETE /api/draft/replay/queue/{i}
pub async fn remove(State(s): State<AppState>, Path(i): Path<usize>) -> Resp {
    cancel_timer(&s);
    mutate(&s, |d| d.remove_game(i)).await
}

#[derive(Deserialize)]
pub struct MoveBody {
    from: usize,
    to: usize,
}

/// POST /api/draft/replay/queue/move
pub async fn move_game(State(s): State<AppState>, Json(b): Json<MoveBody>) -> Resp {
    mutate(&s, |d| d.move_game(b.from, b.to)).await
}

#[derive(Deserialize)]
pub struct FearlessBody {
    value: bool,
}

/// POST /api/draft/replay/fearless — re-prepares the current game (its series bans change).
pub async fn fearless(State(s): State<AppState>, Json(b): Json<FearlessBody>) -> Resp {
    cancel_timer(&s);
    let over = series_format(&s).await;
    mutate(&s, |d| {
        d.set_fearless_with(b.value, over);
        Ok(())
    })
    .await
}

#[derive(Deserialize)]
pub struct PlayBody {
    playing: bool,
    #[serde(default)]
    interval_ms: Option<u32>,
}

/// POST /api/draft/replay/play — auto playback on/off, optional interval (default 30 s).
pub async fn play(State(s): State<AppState>, Json(b): Json<PlayBody>) -> Resp {
    if b.interval_ms
        .is_some_and(|ms| !INTERVAL_RANGE.contains(&ms))
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "bad_interval" })),
        );
    }
    let r = mutate(&s, |d| {
        if let Some(ms) = b.interval_ms {
            d.replay.interval_ms = ms;
        }
        if b.playing {
            d.replay.current.ok_or(DraftError::NoGame)?;
            if d.is_complete() {
                return Err(DraftError::Complete);
            }
            d.replay.next_at_ms = Some(now_ms() + i64::from(d.replay.interval_ms));
        } else {
            d.replay.next_at_ms = None;
        }
        d.replay.playing = b.playing;
        Ok(())
    })
    .await;
    if r.0 == StatusCode::OK {
        if b.playing {
            start_timer(&s);
        } else {
            cancel_timer(&s);
        }
    }
    r
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::api_tests::{call, test_state};
    use crate::draft::replay::LoadedGame;
    use crate::draft::{Format, Side};
    use axum::http::StatusCode;
    use serde_json::json;

    fn game(tag: &str) -> LoadedGame {
        LoadedGame {
            source: format!("{tag}.StormReplay"),
            map: format!("Map {tag}"),
            first_pick: Side::Blue,
            script: (0..16).map(|i| Some(format!("{tag}h{i}"))).collect(),
            pick_players: vec![None; 16],
            blue_players: Default::default(),
            red_players: Default::default(),
        }
    }

    /// Item 26: the temporary file disappears after a successful decoding as after a failure;
    /// the `parse_sem` permit is returned; missing folder → 500 `io`, no panic.
    #[tokio::test]
    async fn decode_removes_temp_file_on_success_and_failure() {
        let dir = std::env::temp_dir().join(format!("draft-decode-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let sem = Arc::new(Semaphore::new(1));
        let aram = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../storm-stats/tests/data/silver-city-aram.StormReplay"
        ))
        .unwrap();
        for bytes in [aram, vec![7u8; 64]] {
            let r = decode_in(sem.clone(), dir.clone(), Bytes::from(bytes)).await;
            assert!(
                r.is_ok(),
                "the decoder answers (storm-stats status), no I/O error"
            );
            assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
            assert_eq!(sem.available_permits(), 1);
        }
        let r = decode_in(sem.clone(), dir.join("nope"), Bytes::from_static(b"x")).await;
        assert_eq!(
            r.err().map(|(st, j)| (st, j.0["error"].clone())),
            Some((StatusCode::INTERNAL_SERVER_ERROR, json!("io")))
        );
        assert_eq!(sem.available_permits(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// M5: during an active cast series, `select` and `fearless` follow the series format,
    /// not `replay.fearless` (in both directions).
    #[tokio::test]
    async fn replay_select_follows_active_series_format() {
        let Some(state) = test_state(None).await else {
            return;
        };
        {
            let mut c = state.cast.write().await;
            c.active = true;
            c.format = Format::Fearless;
            let mut d = state.draft.write().await;
            d.add_game(game("a"));
            d.add_game(game("b"));
            d.replay.fearless = false;
        }
        let (st, _) = call(
            &state,
            "POST",
            "/api/draft/replay/select",
            Some(json!({"index": 1})),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        {
            let d = state.draft.read().await;
            assert_eq!(d.format, Format::Fearless);
            assert!(d.series_bans.contains("ah4"));
        }
        state.cast.write().await.format = Format::Standard;
        let (st, _) = call(
            &state,
            "POST",
            "/api/draft/replay/fearless",
            Some(json!({"value": true})),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        let d = state.draft.read().await;
        assert!(d.replay.fearless);
        assert_eq!(d.format, Format::Standard);
        assert!(d.series_bans.is_empty());
    }
}
