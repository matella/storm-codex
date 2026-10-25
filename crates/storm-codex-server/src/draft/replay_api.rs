//! Routes REST du draft depuis un replay + minuterie de lecture auto. Chaque mutation prend le
//! write-lock une seule fois (donc un « Suivant » manuel et un tick ne se télescopent jamais),
//! persiste, puis diffuse `draft.updated`. Mode ouvert comme le reste de `/api/draft`.
use super::api::persist;
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
use std::time::Duration;

type Resp = (StatusCode, Json<J>);

/// Bornes de l'intervalle de lecture auto (ms) : au moins 10 ms (tests), au plus 1 h.
const INTERVAL_RANGE: std::ops::RangeInclusive<u32> = 10..=3_600_000;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

fn conflict(e: DraftError) -> Resp {
    (StatusCode::CONFLICT, Json(json!({ "error": format!("{e:?}") })))
}

fn ok_state(r: &ReplayState) -> Resp {
    (StatusCode::OK, Json(serde_json::to_value(r).unwrap_or(J::Null)))
}

/// Arrête la minuterie de lecture auto (si elle tourne).
fn cancel_timer(s: &AppState) {
    if let Some(h) = s.replay_timer.lock().ok().and_then(|mut g| g.take()) {
        h.abort();
    }
}

/// (Re)lance la minuterie : un tick révèle une phase toutes les `interval_ms`, jusqu'à la fin.
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

/// GET /api/draft/replay — file complète **avec scripts** (réservé à la page télécommande).
pub async fn get_replay(State(s): State<AppState>) -> Json<ReplayState> {
    Json(s.draft.read().await.replay.clone())
}

/// POST /api/draft/replay/load — corps = octets d'un `.StormReplay` (en-tête `X-Filename` optionnel).
/// Décode uniquement le draft : rien n'est écrit en base. Custom uniquement.
pub async fn load(State(s): State<AppState>, headers: HeaderMap, bytes: Bytes) -> Resp {
    if bytes.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "empty_body" })));
    }
    let name = crate::upload::filename(&headers).unwrap_or_else(|| "replay.StormReplay".into());
    let path = std::env::temp_dir().join(format!("draft-replay-{}.StormReplay", uuid::Uuid::new_v4()));
    if std::fs::write(&path, &bytes).is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "io" })));
    }
    let p2 = path.clone();
    let parsed = tokio::task::spawn_blocking(move || storm_stats::process_replay(&p2, &p2.to_string_lossy())).await;
    let _ = std::fs::remove_file(&path);
    let Ok(out) = parsed else {
        return (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "error": "parse_failed" })));
    };
    match loaded_game_from_output(&out, &name, storm_stats::constants::hero_attribute()) {
        Ok(g) => {
            let r = {
                let mut d = s.draft.write().await;
                d.add_game(g);
                d.public().replay
            };
            persist(&s).await;
            (StatusCode::CREATED, Json(serde_json::to_value(r).unwrap_or(J::Null)))
        }
        Err(e) => {
            let code = match e {
                ReplayError::NotCustom => "not_custom",
                ReplayError::NoDraft => "no_draft",
                ReplayError::Parse(_) => "parse_failed",
            };
            (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "error": code, "message": e.to_string() })))
        }
    }
}

/// Applique une mutation sous un seul write-lock, persiste, répond avec la file publique.
async fn mutate(s: &AppState, f: impl FnOnce(&mut super::DraftState) -> Result<(), DraftError>) -> Resp {
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

/// Après un geste manuel pendant la lecture auto, le prochain tick repart d'un intervalle plein.
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

/// POST /api/draft/replay/next — révèle la phase suivante.
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

/// POST /api/draft/replay/back — annule la dernière phase révélée.
pub async fn back(State(s): State<AppState>) -> Resp {
    let r = mutate(&s, |d| d.back_phase()).await;
    if r.0 == StatusCode::OK {
        rearm_if_playing(&s).await;
    }
    r
}

/// POST /api/draft/replay/reset — repart à l'étape 0 de la partie courante (pause).
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

/// POST /api/draft/replay/select — prépare la partie `index` de la file.
pub async fn select(State(s): State<AppState>, Json(b): Json<IndexBody>) -> Resp {
    cancel_timer(&s);
    mutate(&s, |d| d.select_game(b.index)).await
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

/// POST /api/draft/replay/fearless — re-prépare la partie courante (ses series bans changent).
pub async fn fearless(State(s): State<AppState>, Json(b): Json<FearlessBody>) -> Resp {
    cancel_timer(&s);
    mutate(&s, |d| {
        d.set_fearless(b.value);
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

/// POST /api/draft/replay/play — lecture auto on/off, intervalle optionnel (défaut 30 s).
pub async fn play(State(s): State<AppState>, Json(b): Json<PlayBody>) -> Resp {
    if b.interval_ms.is_some_and(|ms| !INTERVAL_RANGE.contains(&ms)) {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "bad_interval" })));
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
