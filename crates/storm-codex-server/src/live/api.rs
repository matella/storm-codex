//! Live feed routes. `POST /api/live/chunk` follows the raw upload contract (`Bearer` + binary
//! body) with three headers: `X-Session`, `X-Stream` (`tracker` | `game`), `X-Offset`, plus
//! optional `X-Build`. The WS only receives a `live.updated` signal (no state): the filtered view
//! is read through `GET /api/live?scope=…`, so nothing outside the visibility level leaves the server.
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value as J};
use storm_replay::StreamKind;

use crate::live::state::Scope;
use crate::live::{ChunkOutcome, LiveSession, LobbyHint, Status};
use crate::AppState;

const SESSION_MAX_LEN: usize = 128;

fn header<'a>(h: &'a HeaderMap, name: &str) -> Option<&'a str> {
    h.get(name).and_then(|v| v.to_str().ok()).map(str::trim)
}

fn bad(msg: &str) -> (StatusCode, Json<J>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "status": "bad_request", "error": msg })),
    )
}

/// Hints of the current lobby, **only if it belongs to this session** (battlelobby hash =
/// session id, `X-Session` contract). Otherwise `None`: an undetermined "me" is better than a
/// "me" taken from the lobby of another game.
async fn lobby_hint(s: &AppState, session: &str) -> Option<LobbyHint> {
    let lobby = s.lobby.read().await;
    lobby
        .as_ref()
        .filter(|l| l.content_hash == session)
        .map(|l| LobbyHint {
            map: l.map.clone(),
            me_slot: l.me.and_then(|i| i64::try_from(i).ok()),
            humans: l.players.len(),
        })
}

/// `POST /api/live/chunk` — bytes appended to a stream, upload `Bearer`.
pub async fn chunk(
    State(s): State<AppState>,
    headers: HeaderMap,
    bytes: axum::body::Bytes,
) -> (StatusCode, Json<J>) {
    if !crate::upload::token_valide(&s.db, &headers).await {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "status": "unauthorized" })),
        );
    }
    let Some(session) =
        header(&headers, "x-session").filter(|v| !v.is_empty() && v.len() <= SESSION_MAX_LEN)
    else {
        return bad("X-Session manquant ou trop long");
    };
    let kind = match header(&headers, "x-stream") {
        Some("tracker") => StreamKind::Tracker,
        Some("game") => StreamKind::Game,
        _ => return bad("X-Stream doit valoir tracker ou game"),
    };
    let Some(offset) = header(&headers, "x-offset").and_then(|v| v.parse::<u64>().ok()) else {
        return bad("X-Offset manquant ou invalide");
    };
    let build = match header(&headers, "x-build") {
        None => None,
        Some(v) => match v.parse::<u32>() {
            Ok(b) => Some(b),
            Err(_) => return bad("X-Build invalide"),
        },
    };

    let (outcome, new_session) = {
        let mut live = s.live.write().await;
        let new_session = live.as_ref().is_none_or(|l| l.id != session);
        if new_session {
            match LiveSession::new(session.to_owned(), build) {
                Ok(l) => *live = Some(l),
                Err(e) => {
                    tracing::error!("live session impossible: {e}");
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "status": "error" })),
                    );
                }
            }
        }
        let Some(l) = live.as_mut() else {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "status": "error" })),
            );
        };
        (l.apply_chunk(kind, offset, &bytes), new_session)
    };

    let changed =
        new_session || matches!(outcome, ChunkOutcome::Applied { events, .. } if events > 0);
    if changed {
        let _ = s
            .events
            .send(json!({ "type": "live.updated", "session": session }));
    }
    match outcome {
        ChunkOutcome::Applied {
            consumed,
            last_loop,
            events,
        } => (
            StatusCode::OK,
            Json(
                json!({ "status": "applied", "consumed": consumed, "last_loop": last_loop, "events": events }),
            ),
        ),
        ChunkOutcome::Duplicate => (StatusCode::OK, Json(json!({ "status": "duplicate" }))),
        ChunkOutcome::Ignored => (StatusCode::OK, Json(json!({ "status": "unreadable" }))),
        ChunkOutcome::TooLarge => (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({ "status": "too_large" })),
        ),
        ChunkOutcome::Gap { expected } => (
            StatusCode::CONFLICT,
            Json(json!({ "status": "gap", "expected_offset": expected })),
        ),
    }
}

#[derive(Deserialize)]
pub struct EndBody {
    pub session: String,
}

/// `POST /api/live/end` — end of game reported by the client (`replay.details` appears).
pub async fn end(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<EndBody>,
) -> (StatusCode, Json<J>) {
    if !crate::upload::token_valide(&s.db, &headers).await {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "status": "unauthorized" })),
        );
    }
    let ended = {
        let mut live = s.live.write().await;
        match live.as_mut().filter(|l| l.id == body.session) {
            Some(l) => {
                if l.status == Status::Live {
                    l.status = Status::Ended;
                }
                true
            }
            None => false,
        }
    };
    if !ended {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "unknown_session" })),
        );
    }
    let _ = s
        .events
        .send(json!({ "type": "live.updated", "session": body.session }));
    (StatusCode::OK, Json(json!({ "status": "ended" })))
}

#[derive(Deserialize)]
pub struct ViewQuery {
    #[serde(default)]
    pub scope: Scope,
    pub me: Option<i64>,
}

/// `GET /api/live?scope=self|allies|all&me=<playerId>` — filtered view; `204` if no game.
pub async fn get(State(s): State<AppState>, Query(q): Query<ViewQuery>) -> Response {
    let Some(session) = s.live.read().await.as_ref().map(|l| l.id.clone()) else {
        return StatusCode::NO_CONTENT.into_response();
    };
    let hint = lobby_hint(&s, &session).await;
    let live = s.live.read().await;
    match live.as_ref() {
        Some(l) => Json(l.view(q.scope, q.me, hint.as_ref())).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

/// `DELETE /api/live` — closes the current session. Administration write: guarded by
/// `ADMIN_TOKEN` (open mode if it is not configured), like the other management routes.
pub async fn clear(State(s): State<AppState>, headers: HeaderMap) -> (StatusCode, Json<J>) {
    if !crate::manage::is_admin(&headers, &s) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "admin token requis" })),
        );
    }
    *s.live.write().await = None;
    let _ = s
        .events
        .send(json!({ "type": "live.updated", "session": null }));
    (StatusCode::OK, Json(json!({ "ok": true })))
}
