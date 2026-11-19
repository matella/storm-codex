//! 2D viewer (`GET /api/matches/{id}/replay2d`): decodes the archived replay on the fly
//! (storm-replay-viewer) with the same bounded LRU disk cache as `raw.rs` (RAW_CACHE_MAX_BYTES),
//! then merges the current player metadata (name/hero/team/win) from Postgres by toon_handle on
//! every request (the cache never stores that metadata).
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
use storm_replay_viewer::{build_model, player_toons, VIEWER_VERSION};

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

    // The disk cache holds only what the archived replay determines: the viewer model and the
    // playerId → toon mapping. Player metadata (name/hero/team/win) is read from Postgres on every
    // request, hit or miss, because reference activation repairs heroes of an existing match_id in
    // place (project::repair_reference_heroes). The entry is `toons JSON \n model JSON` so a hit
    // splices the players in without parsing the multi-MB model; `-split` keeps entries of older
    // layouts from being read.
    let cache = s
        .cfg
        .raw_cache_dir
        .join(format!("{id}-replay2d-v{VIEWER_VERSION}-split.json"));
    // hit: we touch the file (LRU by access mtime) and serve it with current metadata
    let cached = match tokio::fs::read(&cache).await {
        Ok(bytes) => Entry::parse(bytes),
        Err(_) => None,
    };
    let entry = match cached {
        Some(e) => {
            let _ = filetime_now(&cache);
            e
        }
        None => {
            // miss: decoding on the fly (CPU) off the runtime thread — a single decoding for the
            // geometry AND the playerId→toon mapping (the Replay is consumed in the closure).
            let decoded =
                tokio::task::spawn_blocking(move || -> Result<_, storm_replay_viewer::Error> {
                    let r = Replay::open(file)?;
                    let model = build_model(&r)?;
                    let toons = player_toons(&r)?;
                    Ok((model, toons))
                })
                .await;
            let (model, toons) = match decoded {
                Ok(Ok(v)) => v,
                Ok(Err(e)) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("décodage : {e}"))
                        .into_response()
                }
                Err(_) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, "panic décodage").into_response()
                }
            };
            // An unserialisable model is an error, never a cached `null`.
            let e = match Entry::new(&model, toons) {
                Ok(e) => e,
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("modèle 2D : {e}"),
                    )
                        .into_response()
                }
            };
            let _ = tokio::fs::write(&cache, e.to_bytes()).await;
            enforce_lru(&s.cfg.raw_cache_dir, s.cfg.raw_cache_max_bytes).await;
            e
        }
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

    let players: Vec<serde_json::Value> = entry
        .toons
        .iter()
        .map(|(player_id, toon)| {
            let (name, hero, team, win) = meta_by_toon
                .get(toon)
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

    match serde_json::to_vec(&players) {
        Ok(players) => json_response(entry.splice(&players)),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("joueurs 2D : {e}"),
        )
            .into_response(),
    }
}

/// Why a decoded model cannot be cached and served.
#[derive(Debug, thiserror::Error)]
enum EntryError {
    #[error("serialisation: {0}")]
    Json(#[from] serde_json::Error),
    #[error("the model is not a JSON object without a `players` key")]
    Shape,
}

/// Disk-cache entry: replay-derived data only, never mutable Postgres metadata. `model` holds the
/// serialised viewer model, a non-empty JSON object without a top-level `players` key.
#[derive(Debug)]
struct Entry {
    toons: Vec<(i64, String)>,
    model: Vec<u8>,
}

impl Entry {
    /// Serialises a freshly decoded model (cache miss). The shape check parses the model once,
    /// here, so that hits never have to.
    fn new(model: &impl serde::Serialize, toons: Vec<(i64, String)>) -> Result<Self, EntryError> {
        let model = serde_json::to_vec(model)?;
        match serde_json::from_slice::<HashMap<String, serde::de::IgnoredAny>>(&model) {
            Ok(o) if !o.is_empty() && !o.contains_key("players") => Ok(Self { toons, model }),
            Ok(_) => Err(EntryError::Shape),
            Err(e) => Err(e.into()),
        }
    }

    /// On-disk layout: the toons as one JSON line, then the model. Compact JSON never contains a
    /// raw newline, so the first `\n` is the separator.
    fn to_bytes(&self) -> Vec<u8> {
        let mut out = serde_json::to_vec(&self.toons).unwrap_or_default();
        out.push(b'\n');
        out.extend_from_slice(&self.model);
        out
    }

    /// Reads an entry written by `to_bytes`; anything else is a miss.
    fn parse(mut bytes: Vec<u8>) -> Option<Self> {
        let nl = bytes.iter().position(|&b| b == b'\n')?;
        let toons = serde_json::from_slice(&bytes[..nl]).ok()?;
        let model = bytes.split_off(nl + 1);
        (model.len() > 2 && model.first() == Some(&b'{') && model.last() == Some(&b'}'))
            .then_some(Self { toons, model })
    }

    /// `{model...}` + players → `{"players":[...],model...}`, without parsing the model.
    fn splice(&self, players: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.model.len() + players.len() + 16);
        out.extend_from_slice(b"{\"players\":");
        out.extend_from_slice(players);
        out.push(b',');
        out.extend_from_slice(&self.model[1..]);
        out
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn hit_splices_players_into_the_model() {
        let model = serde_json::json!({"meta": {"mapName": "X"}, "heroes": [1, 2]});
        let e = Entry::new(&model, vec![(1, "1-Hero-1-1".into())]).unwrap();
        let back = Entry::parse(e.to_bytes()).unwrap();
        assert_eq!(back.toons, vec![(1, "1-Hero-1-1".to_string())]);
        let out: serde_json::Value =
            serde_json::from_slice(&back.splice(br#"[{"playerId":1,"hero":"Abathur"}]"#)).unwrap();
        assert_eq!(out["players"][0]["hero"], "Abathur");
        assert_eq!(out["meta"]["mapName"], "X");
        assert_eq!(out["heroes"], serde_json::json!([1, 2]));
    }

    #[test]
    fn strings_with_newlines_survive_the_layout() {
        let model = serde_json::json!({"warnings": ["a\nb"]});
        let e = Entry::new(&model, vec![(1, "t\n".into())]).unwrap();
        let back = Entry::parse(e.to_bytes()).unwrap();
        let out: serde_json::Value = serde_json::from_slice(&back.splice(b"[]")).unwrap();
        assert_eq!(out["warnings"][0], "a\nb");
        assert_eq!(back.toons[0].1, "t\n");
    }

    /// A model that fails to serialise (here a map with non-string keys) is an error to report as
    /// a 500, never an entry; the handler writes the cache only from an `Entry`.
    #[test]
    fn unserialisable_or_misshapen_models_are_rejected() {
        let bad: HashMap<(i32, i32), i32> = [((1, 2), 3)].into();
        assert!(matches!(Entry::new(&bad, vec![]), Err(EntryError::Json(_))));
        for shape in [
            serde_json::Value::Null,
            serde_json::json!([]),
            serde_json::json!({}),
            serde_json::json!({"players": []}),
        ] {
            assert!(Entry::new(&shape, vec![]).is_err(), "{shape}");
        }
    }

    #[test]
    fn foreign_or_truncated_files_are_misses() {
        assert!(Entry::parse(br#"{"model":null,"toons":[]}"#.to_vec()).is_none());
        assert!(Entry::parse(b"[]\nnull".to_vec()).is_none());
        assert!(Entry::parse(b"[]\n{\"meta\":1".to_vec()).is_none());
    }

    /// The real viewer model is a cacheable object: no top-level `players` to collide with.
    #[test]
    fn the_viewer_model_is_cacheable() {
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../storm-replay-viewer/tests/data/silver-city-aram.StormReplay");
        let r = Replay::open(file).unwrap();
        let e = Entry::new(&build_model(&r).unwrap(), player_toons(&r).unwrap()).unwrap();
        let out: serde_json::Value = serde_json::from_slice(&e.splice(b"[]")).unwrap();
        assert!(out["meta"].is_object() && out["heroes"].is_array());
    }
}
