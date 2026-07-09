//! Visionneuse 2D (`GET /api/matches/{id}/replay2d`) : décode le replay archivé à la volée
//! (storm-replay-viewer) avec le même cache disque LRU borné que `raw.rs` (RAW_CACHE_MAX_BYTES),
//! puis fusionne les métadonnées joueurs (name/hero/team/win) depuis Postgres par toon_handle.
//! Jamais de pré-décodage massif — 3e étage de données de la spec.

use crate::raw::{enforce_lru, filetime_now, json_response};
use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::collections::HashMap;
use std::path::PathBuf;
use storm_replay::Replay;
use storm_replay_viewer::{build_model, player_toons, ViewerModel, VIEWER_VERSION};

/// Métadonnées `match_players` jointes par toon_handle : `(name, hero, team, win)`.
type PlayerMeta = (Option<String>, Option<String>, Option<i32>, Option<bool>);
/// Ligne brute `match_players` (toon_handle + métadonnées).
type PlayerMetaRow = (
    String,
    Option<String>,
    Option<String>,
    Option<i32>,
    Option<bool>,
);

pub async fn get_replay2d(State(s): State<AppState>, Path(id): Path<i64>) -> Response {
    // fichier archivé d'un upload ayant produit ce match (même requête que raw.rs)
    let archived: Option<String> = sqlx::query_scalar(
        "SELECT archived_path FROM uploads WHERE match_id = $1 AND status = 'parsed'
         AND archived_path IS NOT NULL LIMIT 1",
    )
    .bind(id)
    .fetch_optional(&s.db)
    .await
    .ok()
    .flatten();
    let Some(archived) = archived else {
        return (
            StatusCode::NOT_FOUND,
            "match ou fichier archivé introuvable",
        )
            .into_response();
    };

    // Clé de cache = match_id + VIEWER_VERSION seulement. Le payload embarque pourtant des
    // métadonnées match_players mutables (name/hero/team/win) sans y être clé — sûr UNIQUEMENT
    // parce que reprocesser un replay alloue un match_id neuf (project.rs : DELETE par fingerprint,
    // cascade → match_players, puis INSERT … RETURNING id), jamais un UPDATE in-place des joueurs
    // d'un match_id existant. Si une édition in-place des métadonnées joueurs d'un match_id existant
    // est un jour ajoutée, invalider/évincer cette entrée de cache à cet endroit-là.
    let cache = s
        .cfg
        .raw_cache_dir
        .join(format!("{id}-replay2d-v{VIEWER_VERSION}.json"));
    // hit : on touche le fichier (LRU par mtime d'accès) et on sert
    if let Ok(bytes) = tokio::fs::read(&cache).await {
        let _ = filetime_now(&cache);
        return json_response(bytes);
    }

    // miss : décodage à la volée (CPU) hors thread runtime — un seul décodage pour la géométrie
    // ET le mapping playerId→toon (le Replay est consommé dans la closure).
    let decoded = tokio::task::spawn_blocking(move || -> Result<_, storm_replay_viewer::Error> {
        let r = Replay::open(PathBuf::from(archived))?;
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

    // métadonnées joueurs (async, hors blocking) : toon_handle → (name, hero, team, win)
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

/// Insère `players` dans le modèle sérialisé : `{meta, heroes, deaths, warnings}` → `{meta,
/// players, heroes, deaths, warnings}`.
fn merge(model: &ViewerModel, players: serde_json::Value) -> serde_json::Value {
    let mut v = serde_json::to_value(model).unwrap_or_default();
    if let Some(o) = v.as_object_mut() {
        o.insert("players".into(), players);
    }
    v
}
