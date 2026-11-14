//! 2D viewer (`GET /api/matches/{id}/replay2d`): decodes the archived replay on the fly
//! (storm-replay-viewer) with the same bounded LRU disk cache as `raw.rs` (RAW_CACHE_MAX_BYTES),
//! then merges the player metadata (name/hero/team/win) from Postgres by toon_handle.
//! Never any mass pre-decoding — the spec's 3rd data tier.

use crate::raw::{enforce_lru, filetime_now, json_response};
use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::collections::HashMap;
use storm_replay::Replay;
use storm_replay_viewer::{build_model, player_toons, ViewerModel, VIEWER_VERSION};

/// `match_players` metadata joined by toon_handle: `(name, hero, team, win)`.
type PlayerMeta = (Option<String>, Option<String>, Option<i32>, Option<bool>);
/// Raw `match_players` row (toon_handle + metadata).
type PlayerMetaRow = (
    String,
    Option<String>,
    Option<String>,
    Option<i32>,
    Option<bool>,
);

pub async fn get_replay2d(State(s): State<AppState>, Path(id): Path<i64>) -> Response {
    // archived file of an upload that produced this match (same query as raw.rs)
    let archived: Option<String> = sqlx::query_scalar(
        "SELECT archived_path FROM uploads WHERE match_id = $1 AND status = 'parsed'
         AND archived_path IS NOT NULL LIMIT 1",
    )
    .bind(id)
    .fetch_optional(&s.db)
    .await
    .ok()
    .flatten();
    let Some(file) = archived.map(|a| s.cfg.archived_file(&a)) else {
        return (
            StatusCode::NOT_FOUND,
            "match ou fichier archivé introuvable",
        )
            .into_response();
    };

    // Cache key = match_id + VIEWER_VERSION only. Yet the payload embeds mutable
    // match_players metadata (name/hero/team/win) without it being part of the key — safe ONLY
    // because re-processing a replay allocates a new match_id (project.rs: DELETE by fingerprint,
    // cascade → match_players, then INSERT … RETURNING id), never an in-place UPDATE of the players
    // of an existing match_id. If an in-place edit of the player metadata of an existing match_id
    // is ever added, invalidate/evict this cache entry at that place.
    let cache = s
        .cfg
        .raw_cache_dir
        .join(format!("{id}-replay2d-v{VIEWER_VERSION}.json"));
    // hit: we touch the file (LRU by access mtime) and serve it
    if let Ok(bytes) = tokio::fs::read(&cache).await {
        let _ = filetime_now(&cache);
        return json_response(bytes);
    }

    // miss: decoding on the fly (CPU) off the runtime thread — a single decoding for the geometry
    // AND the playerId→toon mapping (the Replay is consumed in the closure).
    let decoded = tokio::task::spawn_blocking(move || -> Result<_, storm_replay_viewer::Error> {
        let r = Replay::open(file)?;
        let model = build_model(&r)?;
        let toons = player_toons(&r)?;
        Ok((model, toons))
    })
    .await;
    let (model, toons) = match decoded {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("décodage : {e}")).into_response()
        }
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "panic décodage").into_response(),
    };

    // player metadata (async, outside blocking): toon_handle → (name, hero, team, win)
    let meta_by_toon: HashMap<String, PlayerMeta> = sqlx::query_as::<_, PlayerMetaRow>(
        "SELECT toon_handle, name, hero, team, win FROM match_players WHERE match_id = $1",
    )
    .bind(id)
    .fetch_all(&s.db)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|(toon, name, hero, team, win)| (toon, (name, hero, team, win)))
    .collect();

    let players: Vec<serde_json::Value> = toons
        .into_iter()
        .map(|(player_id, toon)| {
            let (name, hero, team, win) = meta_by_toon
                .get(&toon)
                .cloned()
                .unwrap_or((None, None, None, None));
            serde_json::json!({
                "playerId": player_id,
                "name": name,
                "hero": hero,
                "team": team,
                "win": win,
            })
        })
        .collect();

    let json = merge(&model, serde_json::Value::Array(players));
    let bytes = serde_json::to_vec(&json).unwrap_or_default();
    let _ = tokio::fs::write(&cache, &bytes).await;
    enforce_lru(&s.cfg.raw_cache_dir, s.cfg.raw_cache_max_bytes).await;
    json_response(bytes)
}

/// Inserts `players` into the serialised model: `{meta, heroes, deaths, warnings}` → `{meta,
/// players, heroes, deaths, warnings}`.
fn merge(model: &ViewerModel, players: serde_json::Value) -> serde_json::Value {
    let mut v = serde_json::to_value(model).unwrap_or_default();
    if let Some(o) = v.as_object_mut() {
        o.insert("players".into(), players);
    }
    v
}
