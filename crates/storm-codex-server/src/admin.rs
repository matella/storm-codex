//! Admin (`/api/admin/*`): upload token management, idempotent re-process (driven by
//! `parser_version`), health/observability. Protected by `ADMIN_TOKEN` (Bearer).

use crate::{project, upload::sha256_hex, AppState, PARSER_VERSION};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use rand::RngCore;
use serde::Deserialize;
use serde_json::Value as J;

fn is_admin(headers: &HeaderMap, state: &AppState) -> bool {
    crate::auth::is_admin(headers, state.cfg.admin_token.as_deref())
}

fn forbidden() -> (StatusCode, Json<J>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({"error": "admin token requis"})),
    )
}

#[derive(Deserialize)]
pub struct NewToken {
    name: String,
}

/// POST /api/admin/tokens — creates a named token. The clear value is only returned here (never stored).
pub async fn create_token(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<NewToken>,
) -> (StatusCode, Json<J>) {
    if !is_admin(&headers, &s) {
        return forbidden();
    }
    let mut raw = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut raw);
    let token = hex::encode(raw);
    let hash = sha256_hex(token.as_bytes());
    match sqlx::query_scalar::<_, i64>(
        "INSERT INTO upload_tokens (name, token_hash) VALUES ($1, $2) RETURNING id",
    )
    .bind(&body.name)
    .bind(&hash)
    .fetch_one(&s.db)
    .await
    {
        Ok(id) => (
            StatusCode::CREATED,
            Json(serde_json::json!({"id": id, "name": body.name, "token": token})),
        ),
        Err(e) => {
            tracing::error!("create_token: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "db"})),
            )
        }
    }
}

/// DELETE /api/admin/tokens/{id} — revokes a token.
pub async fn revoke_token(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> (StatusCode, Json<J>) {
    if !is_admin(&headers, &s) {
        return forbidden();
    }
    // `COALESCE`: revoking twice keeps the date of the first revocation (idempotent).
    match sqlx::query(
        "UPDATE upload_tokens SET revoked_at = COALESCE(revoked_at, now()) WHERE id = $1",
    )
    .bind(id)
    .execute(&s.db)
    .await
    {
        Ok(r) if r.rows_affected() == 0 => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "token inconnu"})),
        ),
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({"revoked": id}))),
        Err(e) => {
            tracing::error!("revoke_token {id}: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "db"})),
            )
        }
    }
}

/// GET /api/admin/uploads — health: count per status + failures per class + latest failures.
pub async fn uploads_health(
    State(s): State<AppState>,
    headers: HeaderMap,
) -> (StatusCode, Json<J>) {
    if !is_admin(&headers, &s) {
        return forbidden();
    }
    let v: J = sqlx::query_scalar(
        "SELECT jsonb_build_object(
            'by_status', COALESCE((SELECT jsonb_object_agg(status, n) FROM
                (SELECT status, count(*) n FROM uploads GROUP BY status) a), '{}'::jsonb),
            'by_error_class', COALESCE((SELECT jsonb_object_agg(error_class, n) FROM
                (SELECT error_class, count(*) n FROM uploads
                 WHERE error_class IS NOT NULL GROUP BY error_class) b), '{}'::jsonb),
            'recent_failures', COALESCE((SELECT jsonb_agg(f) FROM
                (SELECT filename, error_class, error_msg, created_at FROM uploads
                 WHERE status='parse_failed' ORDER BY created_at DESC LIMIT 20) f), '[]'::jsonb),
            'parser_version', $1::int)",
    )
    .bind(PARSER_VERSION)
    .fetch_one(&s.db)
    .await
    .unwrap_or_else(|_| serde_json::json!({"error": "db"}));
    (StatusCode::OK, Json(v))
}

/// POST /api/admin/reprocess — re-parses the archived files whose `parser_version` is outdated
/// (or all of them). Idempotent (project_match replaces). Runs in the background.
pub async fn reprocess(State(s): State<AppState>, headers: HeaderMap) -> (StatusCode, Json<J>) {
    if !is_admin(&headers, &s) {
        return forbidden();
    }
    // uploads to resume: parsed with an outdated parser version, or retryable failures
    let targets: Vec<(i64, String)> = match sqlx::query_as(
        "SELECT id, archived_path FROM uploads
         WHERE archived_path IS NOT NULL
           AND (status = 'parse_failed' OR (status = 'parsed' AND parser_version < $1))",
    )
    .bind(PARSER_VERSION)
    .fetch_all(&s.db)
    .await
    {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("reprocess: selection failed: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "db"})),
            );
        }
    };

    let queued = targets.len();
    let state = s.clone();
    tokio::spawn(async move {
        for (upload_id, path) in targets {
            let _permit = state.parse_sem.acquire().await;
            reprocess_one(&state, upload_id, &path).await;
        }
        tracing::info!("re-process done ({queued} files)");
        // scouting replays (dedicated tables): same parser_version criterion, then recompute the facts
        let n = crate::scouting::api::reprocess_stale(&state).await;
        tracing::info!("scouting re-process done ({n} replays)");
    });
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({"queued": queued})),
    )
}

async fn reprocess_one(state: &AppState, upload_id: i64, path: &str) {
    let p = std::path::PathBuf::from(path);
    let fname = path.to_owned();
    let out =
        match tokio::task::spawn_blocking(move || storm_stats::process_replay(&p, &fname)).await {
            Ok(out) => out,
            Err(e) => {
                tracing::error!("reprocess upload {upload_id}: worker failed: {e}");
                return;
            }
        };
    if out.status != 1 {
        return; // the failure stays classified as is
    }
    if let Some(fp) = crate::upload::game_fingerprint(&out) {
        match project::project_match(&state.db, &fp, PARSER_VERSION, &out).await {
            Ok(match_id) => {
                if let Err(e) = sqlx::query(
                    "UPDATE uploads SET status='parsed', parser_version=$2, match_id=$3,
                     error_class=NULL, error_msg=NULL, parsed_at=now() WHERE id=$1",
                )
                .bind(upload_id)
                .bind(PARSER_VERSION)
                .bind(match_id)
                .execute(&state.db)
                .await
                {
                    tracing::error!("reprocess upload {upload_id}: status not written: {e}");
                }
            }
            Err(e) => tracing::error!("reprocess upload {upload_id}: projection: {e}"),
        }
    }
}
