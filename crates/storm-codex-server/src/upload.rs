//! Pipeline d'upload (parser.js du jalon 3) : token → archive d'abord → pool de parse →
//! projection → statut. Sémantique : attend le résultat jusqu'à 2 s, sinon `202 accepted`.

use crate::{project, AppState, PARSER_VERSION};
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::Duration;

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

/// Fingerprint de partie (compat overlay) : SHA-256("date|map|length|toonsTriés").
pub(crate) fn game_fingerprint(out: &storm_stats::Output) -> Option<String> {
    let m = out.match_.as_ref()?;
    let players = out.players.as_ref()?;
    let date = m.get("date").and_then(|v| v.as_str())?;
    let map = m.get("map").and_then(|v| v.as_str())?;
    // `${length}` JS : entier sans .0, sinon décimal court (f64 Display == String JS ici)
    let length = m.get("length").and_then(|v| v.as_f64())?;
    let mut toons: Vec<&str> = players.keys().map(String::as_str).collect();
    toons.sort_unstable();
    let src = format!("{date}|{map}|{length}|{}", toons.join(","));
    Some(sha256_hex(src.as_bytes()))
}

/// storm-stats statut ≠ 1 → classe d'erreur typée (visible en Admin).
fn reject_class(status: i64) -> &'static str {
    match status {
        0 => "unsupported_mode",   // brawl
        -3 => "unsupported_map",   // carte hors table (hors EXTRA_MAPS)
        -4 => "computer_player",
        -5 => "incomplete",
        -6 => "too_old",
        _ => "parse_failed",
    }
}

#[derive(serde::Serialize)]
pub struct UploadResponse {
    pub status: String, // parsed | duplicate | parse_failed | accepted
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_class: Option<String>,
}

fn bearer(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.trim().to_owned())
}

pub async fn upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    bytes: Bytes,
) -> (StatusCode, Json<UploadResponse>) {
    // 1. authentification par token (token_hash = SHA-256 du token en clair)
    let token_id: Option<i64> = match bearer(&headers) {
        Some(tok) => {
            let h = sha256_hex(tok.as_bytes());
            sqlx::query_scalar(
                "SELECT id FROM upload_tokens WHERE token_hash = $1 AND revoked_at IS NULL",
            )
            .bind(&h)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten()
        }
        None => None,
    };
    let Some(token_id) = token_id else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(UploadResponse { status: "unauthorized".into(), match_id: None, error_class: None }),
        );
    };

    // 2. dédup fichier (hash de contenu) — réponse immédiate si déjà traité
    let content_hash = sha256_hex(&bytes);
    if let Ok(Some(st)) = sqlx::query_scalar::<_, String>(
        "SELECT status FROM uploads WHERE fingerprint = $1",
    )
    .bind(&content_hash)
    .fetch_optional(&state.db)
    .await
    {
        if st == "parsed" || st == "duplicate" {
            return (
                StatusCode::OK,
                Json(UploadResponse { status: "duplicate".into(), match_id: None, error_class: None }),
            );
        }
    }

    // 3. archive d'abord (source de vérité), puis ligne uploads(pending)
    let archived = state.cfg.archive_dir.join(format!("{content_hash}.StormReplay"));
    if let Err(e) = tokio::fs::write(&archived, &bytes).await {
        tracing::error!("archivage : {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(UploadResponse { status: "error".into(), match_id: None, error_class: Some("io".into()) }),
        );
    }
    let upload_id: i64 = match sqlx::query_scalar(
        "INSERT INTO uploads (token_id, filename, fingerprint, archived_path, status)
         VALUES ($1,$2,$3,$4,'pending')
         ON CONFLICT (fingerprint) DO UPDATE SET status = 'pending'
         RETURNING id",
    )
    .bind(token_id)
    .bind(filename(&headers).unwrap_or_else(|| format!("{content_hash}.StormReplay")))
    .bind(&content_hash)
    .bind(archived.to_string_lossy().as_ref())
    .fetch_one(&state.db)
    .await
    {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("insert upload : {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(UploadResponse { status: "error".into(), match_id: None, error_class: Some("db".into()) }),
            );
        }
    };

    // 4. parse en pool (jamais sur le thread HTTP) ; pool saturé → 202 accepted
    let permit = match state.parse_sem.clone().try_acquire_owned() {
        Ok(p) => p,
        Err(_) => {
            // backfill : on accepte, le parse suivra dès qu'un worker se libère
            let st = state.clone();
            tokio::spawn(async move {
                if let Ok(p) = st.parse_sem.clone().acquire_owned().await {
                    run_parse(st, upload_id, archived, p).await;
                }
            });
            return (
                StatusCode::ACCEPTED,
                Json(UploadResponse { status: "accepted".into(), match_id: None, error_class: None }),
            );
        }
    };

    // attend le résultat jusqu'à 2 s (typique < 0,5 s)
    let st = state.clone();
    let handle = tokio::spawn(async move { run_parse(st, upload_id, archived, permit).await });
    match tokio::time::timeout(Duration::from_secs(2), handle).await {
        Ok(Ok(outcome)) => outcome.into_response(),
        _ => (
            StatusCode::ACCEPTED,
            Json(UploadResponse { status: "accepted".into(), match_id: None, error_class: None }),
        ),
    }
}

struct Outcome {
    status: String,
    match_id: Option<i64>,
    error_class: Option<String>,
}
impl Outcome {
    fn into_response(self) -> (StatusCode, Json<UploadResponse>) {
        (
            StatusCode::OK,
            Json(UploadResponse {
                status: self.status,
                match_id: self.match_id,
                error_class: self.error_class,
            }),
        )
    }
}

/// Parse CPU-bound (spawn_blocking) + projection + mise à jour `uploads`. Diffuse l'event WS.
async fn run_parse(
    state: AppState,
    upload_id: i64,
    archived: PathBuf,
    _permit: tokio::sync::OwnedSemaphorePermit,
) -> Outcome {
    let path = archived.clone();
    // catch_unwind via spawn_blocking : un panic worker n'abat pas le serveur
    let parsed = tokio::task::spawn_blocking(move || {
        storm_stats::process_replay(&path, &path.to_string_lossy())
    })
    .await;

    let out = match parsed {
        Ok(out) => out,
        Err(_) => {
            mark_failed(&state.db, upload_id, "panic", "worker panic").await;
            return Outcome { status: "parse_failed".into(), match_id: None, error_class: Some("panic".into()) };
        }
    };

    if out.status != 1 {
        let class = reject_class(out.status);
        mark_failed(&state.db, upload_id, class, &format!("statut storm-stats {}", out.status)).await;
        return Outcome { status: "parse_failed".into(), match_id: None, error_class: Some(class.into()) };
    }

    let Some(fp) = game_fingerprint(&out) else {
        mark_failed(&state.db, upload_id, "no_fingerprint", "fingerprint indisponible").await;
        return Outcome { status: "parse_failed".into(), match_id: None, error_class: Some("no_fingerprint".into()) };
    };
    let build = out
        .match_
        .as_ref()
        .and_then(|m| m.get("version"))
        .and_then(|v| v.get("m_build"))
        .and_then(|v| v.as_i64())
        .map(|v| v as i32);

    match project::project_match(&state.db, &fp, PARSER_VERSION, &out).await {
        Ok(match_id) => {
            let _ = sqlx::query(
                "UPDATE uploads SET status='parsed', parser_version=$2, build=$3, match_id=$4,
                 error_class=NULL, error_msg=NULL, parsed_at=now() WHERE id=$1",
            )
            .bind(upload_id)
            .bind(PARSER_VERSION)
            .bind(build)
            .bind(match_id)
            .execute(&state.db)
            .await;
            // push temps réel (jalon 3 T5)
            let _ = state.events.send(serde_json::json!({
                "type": "match.parsed",
                "match_id": match_id,
                "map": out.match_.as_ref().and_then(|m| m.get("map")).cloned(),
            }));
            Outcome { status: "parsed".into(), match_id: Some(match_id), error_class: None }
        }
        Err(e) => {
            mark_failed(&state.db, upload_id, "projection", &e.to_string()).await;
            Outcome { status: "parse_failed".into(), match_id: None, error_class: Some("projection".into()) }
        }
    }
}

async fn mark_failed(db: &sqlx::PgPool, upload_id: i64, class: &str, msg: &str) {
    let _ = sqlx::query(
        "UPDATE uploads SET status='parse_failed', error_class=$2, error_msg=$3, parsed_at=now()
         WHERE id=$1",
    )
    .bind(upload_id)
    .bind(class)
    .bind(msg)
    .execute(db)
    .await;
}

fn filename(headers: &HeaderMap) -> Option<String> {
    headers.get("x-filename").and_then(|v| v.to_str().ok()).map(str::to_owned)
}
