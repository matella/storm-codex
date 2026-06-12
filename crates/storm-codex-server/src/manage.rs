//! Définitions manuelles (équipes, collections) — recréées dans l'UI (parité SotS). CRUD
//! protégé en écriture par `ADMIN_TOKEN`. Lecture publique (LAN/Tailscale).

use crate::AppState;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::Value as J;

fn is_admin(h: &HeaderMap, s: &AppState) -> bool {
    h.get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        == Some(s.cfg.admin_token.as_str())
}
fn forbidden() -> (StatusCode, Json<J>) {
    (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "admin token requis"})))
}
fn db_err(e: sqlx::Error) -> (StatusCode, Json<J>) {
    tracing::error!("manage : {e}");
    (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "db"})))
}

#[derive(Deserialize)]
pub struct NewTeam {
    name: String,
    #[serde(default)]
    roster: Vec<String>,
}

/// GET /api/teams
pub async fn list_teams(State(s): State<AppState>) -> Result<Json<J>, (StatusCode, Json<J>)> {
    let v: J = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(jsonb_build_object('id',id,'name',name,'roster',roster)
         ORDER BY name), '[]'::jsonb) FROM teams",
    )
    .fetch_one(&s.db)
    .await
    .map_err(db_err)?;
    Ok(Json(v))
}

/// POST /api/teams (admin)
pub async fn create_team(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(b): Json<NewTeam>,
) -> (StatusCode, Json<J>) {
    if !is_admin(&headers, &s) {
        return forbidden();
    }
    let roster = serde_json::to_value(&b.roster).unwrap_or_else(|_| serde_json::json!([]));
    match sqlx::query_scalar::<_, i64>(
        "INSERT INTO teams (name, roster) VALUES ($1, $2) RETURNING id",
    )
    .bind(&b.name)
    .bind(&roster)
    .fetch_one(&s.db)
    .await
    {
        Ok(id) => (StatusCode::CREATED, Json(serde_json::json!({"id": id}))),
        Err(e) => db_err(e),
    }
}

/// DELETE /api/teams/{id} (admin)
pub async fn delete_team(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> (StatusCode, Json<J>) {
    if !is_admin(&headers, &s) {
        return forbidden();
    }
    let _ = sqlx::query("DELETE FROM teams WHERE id = $1").bind(id).execute(&s.db).await;
    (StatusCode::OK, Json(serde_json::json!({"deleted": id})))
}

#[derive(Deserialize)]
pub struct NewCollection {
    name: String,
    #[serde(default)]
    match_ids: Vec<i64>,
}

/// GET /api/collections
pub async fn list_collections(State(s): State<AppState>) -> Result<Json<J>, (StatusCode, Json<J>)> {
    let v: J = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(jsonb_build_object('id',id,'name',name,
         'count',jsonb_array_length(match_ids)) ORDER BY name), '[]'::jsonb) FROM collections",
    )
    .fetch_one(&s.db)
    .await
    .map_err(db_err)?;
    Ok(Json(v))
}

/// POST /api/collections (admin)
pub async fn create_collection(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(b): Json<NewCollection>,
) -> (StatusCode, Json<J>) {
    if !is_admin(&headers, &s) {
        return forbidden();
    }
    let ids = serde_json::to_value(&b.match_ids).unwrap_or_else(|_| serde_json::json!([]));
    match sqlx::query_scalar::<_, i64>(
        "INSERT INTO collections (name, match_ids) VALUES ($1, $2) RETURNING id",
    )
    .bind(&b.name)
    .bind(&ids)
    .fetch_one(&s.db)
    .await
    {
        Ok(id) => (StatusCode::CREATED, Json(serde_json::json!({"id": id}))),
        Err(e) => db_err(e),
    }
}

/// DELETE /api/collections/{id} (admin)
pub async fn delete_collection(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> (StatusCode, Json<J>) {
    if !is_admin(&headers, &s) {
        return forbidden();
    }
    let _ = sqlx::query("DELETE FROM collections WHERE id = $1").bind(id).execute(&s.db).await;
    (StatusCode::OK, Json(serde_json::json!({"deleted": id})))
}

/// GET /api/trends — winrate & parties agrégés par build (proxy de patch), récents d'abord.
pub async fn trends(State(s): State<AppState>) -> Result<Json<J>, (StatusCode, Json<J>)> {
    let v: J = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(t ORDER BY t.build DESC), '[]'::jsonb) FROM (
            SELECT build, count(*) AS games,
                   count(*) FILTER (WHERE winner = 0) AS blue_wins,
                   round(avg(length)::numeric, 0) AS avg_length
            FROM matches WHERE build IS NOT NULL GROUP BY build) t",
    )
    .fetch_one(&s.db)
    .await
    .map_err(db_err)?;
    Ok(Json(v))
}
