//! Dump décodé à la demande (`GET /api/matches/{id}/raw?stream=…`) : décode le fichier archivé
//! à la volée (storm-replay) avec un cache disque **LRU borné** (RAW_CACHE_MAX_BYTES). Jamais
//! de pré-décodage massif — 3e étage de données de la spec.

use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::Value as J;
use std::path::PathBuf;
use storm_replay::{Replay, Value};

#[derive(Deserialize)]
pub struct RawQuery {
    #[serde(default = "default_stream")]
    stream: String,
}
fn default_stream() -> String {
    "tracker".into()
}

const STREAMS: [&str; 7] = [
    "header",
    "details",
    "initdata",
    "attributes",
    "tracker",
    "game",
    "message",
];

pub async fn get_raw(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<RawQuery>,
) -> Response {
    if !STREAMS.contains(&q.stream.as_str()) {
        return (
            StatusCode::BAD_REQUEST,
            format!("stream inconnu : {}", q.stream),
        )
            .into_response();
    }
    // fichier archivé d'un upload ayant produit ce match
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

    let cache = s.cfg.raw_cache_dir.join(format!("{id}-{}.json", q.stream));
    // hit : on touche le fichier (LRU par mtime d'accès) et on sert
    if let Ok(bytes) = tokio::fs::read(&cache).await {
        let _ = filetime_now(&cache);
        return json_response(bytes);
    }

    // miss : décodage à la volée (CPU) hors thread runtime
    let stream = q.stream.clone();
    let decoded =
        tokio::task::spawn_blocking(move || decode_stream(&PathBuf::from(archived), &stream)).await;
    let json = match decoded {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("décodage : {e}")).into_response()
        }
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "panic décodage").into_response(),
    };
    let bytes = serde_json::to_vec(&json).unwrap_or_default();
    let _ = tokio::fs::write(&cache, &bytes).await;
    enforce_lru(&s.cfg.raw_cache_dir, s.cfg.raw_cache_max_bytes).await;
    json_response(bytes)
}

fn json_response(bytes: Vec<u8>) -> Response {
    ([(header::CONTENT_TYPE, "application/json")], bytes).into_response()
}

fn filetime_now(path: &std::path::Path) -> std::io::Result<()> {
    // « touch » : réécrit l'heure d'accès/modif pour le classement LRU
    let f = std::fs::OpenOptions::new().append(true).open(path)?;
    f.set_modified(std::time::SystemTime::now())
}

/// Éviction LRU : tant que la somme des tailles dépasse `max`, supprime le plus ancien (mtime).
async fn enforce_lru(dir: &std::path::Path, max: u64) {
    let dir = dir.to_path_buf();
    let _ = tokio::task::spawn_blocking(move || {
        let mut files: Vec<(std::path::PathBuf, u64, std::time::SystemTime)> =
            std::fs::read_dir(&dir)
                .into_iter()
                .flatten()
                .filter_map(|e| e.ok())
                .filter_map(|e| {
                    let m = e.metadata().ok()?;
                    if m.is_file() {
                        Some((e.path(), m.len(), m.modified().ok()?))
                    } else {
                        None
                    }
                })
                .collect();
        let mut total: u64 = files.iter().map(|(_, n, _)| *n).sum();
        if total <= max {
            return;
        }
        files.sort_by_key(|(_, _, t)| *t); // plus ancien d'abord
        for (path, size, _) in files {
            if total <= max {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total = total.saturating_sub(size);
            }
        }
    })
    .await;
}

fn decode_stream(path: &std::path::Path, stream: &str) -> Result<J, String> {
    let replay = Replay::open(path).map_err(|e| e.to_string())?;
    let to_err = |e: storm_replay::Error| e.to_string();
    Ok(match stream {
        "header" => value_to_json(&replay.header_raw),
        "details" => value_to_json(&replay.details_raw().map_err(to_err)?),
        "initdata" => value_to_json(&replay.initdata_raw().map_err(to_err)?),
        "attributes" => {
            let a = replay.attributes().map_err(to_err)?;
            // structure légère (les attributes sont peu volumineux)
            serde_json::json!({"source": a.source, "mapNamespace": a.map_namespace})
        }
        "tracker" => events_to_json(replay.tracker_events().map_err(to_err)?),
        "game" => events_to_json(replay.game_events().map_err(to_err)?),
        "message" => events_to_json(replay.message_events().map_err(to_err)?),
        _ => return Err(format!("stream inconnu {stream}")),
    })
}

fn events_to_json(events: Vec<Value>) -> J {
    J::Array(events.iter().map(value_to_json).collect())
}

/// `storm_replay::Value` → JSON (blobs en UTF-8 lossy, bitarrays neutralisés).
fn value_to_json(v: &Value) -> J {
    match v {
        Value::Null => J::Null,
        Value::Int(i) => J::from(*i),
        Value::Bool(b) => J::from(*b),
        Value::Real(f) => serde_json::Number::from_f64(*f).map_or(J::Null, J::Number),
        Value::Blob(b) => J::from(String::from_utf8_lossy(b).into_owned()),
        Value::Str(s) => J::from(s.as_ref()),
        Value::Fourcc(b) => J::from(String::from_utf8_lossy(b).into_owned()),
        Value::Array(items) => J::Array(items.iter().map(value_to_json).collect()),
        Value::BitArrayBytes { bits, .. } => serde_json::json!([bits, null]),
        Value::BitArrayInt { bits, .. } => serde_json::json!([bits, null]),
        Value::Struct(fields) => J::Object(
            fields
                .iter()
                .map(|(n, v)| (n.to_string(), value_to_json(v)))
                .collect(),
        ),
    }
}
