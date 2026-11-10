//! Manual definitions (teams, collections) — recreated in the UI (SotS parity). CRUD
//! write-protected by `ADMIN_TOKEN`. Public read (LAN/Tailscale).

use crate::AppState;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::Value as J;

pub(crate) fn is_admin(h: &HeaderMap, s: &AppState) -> bool {
    crate::auth::is_admin(h, s.cfg.admin_token.as_deref())
}
fn forbidden() -> (StatusCode, Json<J>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({"error": "admin token requis"})),
    )
}
fn db_err(e: sqlx::Error) -> (StatusCode, Json<J>) {
    tracing::error!("manage: {e}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": "db"})),
    )
}

// ── Settings (operator_names…) ──────────────────────────────────────────────
/// GET /api/settings — public settings (read). Today: `operator_names`.
pub async fn get_settings(State(s): State<AppState>) -> Result<Json<J>, (StatusCode, Json<J>)> {
    let mut v: J = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_object_agg(key, value), '{}'::jsonb) FROM app_settings",
    )
    .fetch_one(&s.db)
    .await
    .map_err(db_err)?;
    // `admin_open`: true = no admin auth required (ADMIN_TOKEN missing) → the front hides the
    // token field and sends its writes without a Bearer.
    if let Some(obj) = v.as_object_mut() {
        obj.insert("admin_open".into(), J::Bool(s.cfg.admin_token.is_none()));
    }
    Ok(Json(v))
}

#[derive(Deserialize)]
pub struct SettingsPatch {
    operator_names: Vec<String>,
}

/// PUT /api/admin/settings (admin) — updates the list of operator names.
pub async fn put_settings(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(b): Json<SettingsPatch>,
) -> (StatusCode, Json<J>) {
    if !is_admin(&headers, &s) {
        return forbidden();
    }
    // normalise: trim, non-empty, dedup preserving the order
    let mut names: Vec<String> = Vec::new();
    for n in b.operator_names {
        let n = n.trim().to_string();
        if !n.is_empty() && !names.contains(&n) {
            names.push(n);
        }
    }
    let value = serde_json::to_value(&names).unwrap_or_else(|_| serde_json::json!([]));
    match sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES ('operator_names', $1)
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
    )
    .bind(&value)
    .execute(&s.db)
    .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({"operator_names": names})),
        ),
        Err(e) => {
            let (c, j) = db_err(e);
            (c, j)
        }
    }
}

/// PUT /api/admin/minimap-anchors (admin) — persists the minimap calibration anchors per map
/// (`{ "<slug>": { "blue": [x,y], "red": [x,y] } }`) in `app_settings.minimap_anchors`. Consumed
/// by the 2D viewer (overrides the baked defaults). Body = the complete object (replacement).
pub async fn put_minimap_anchors(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(anchors): Json<J>,
) -> (StatusCode, Json<J>) {
    if !is_admin(&headers, &s) {
        return forbidden();
    }
    if !anchors.is_object() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "objet attendu"})),
        );
    }
    match sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES ('minimap_anchors', $1)
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
    )
    .bind(&anchors)
    .execute(&s.db)
    .await
    {
        Ok(_) => (StatusCode::OK, Json(anchors)),
        Err(e) => db_err(e),
    }
}

#[derive(Deserialize)]
pub struct NewTeam {
    name: String,
    #[serde(default)]
    roster: Vec<String>,
    #[serde(default)]
    league: Option<String>,
}

/// GET /api/teams — includes the league (grouping).
pub async fn list_teams(State(s): State<AppState>) -> Result<Json<J>, (StatusCode, Json<J>)> {
    let v: J = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(jsonb_build_object('id',id,'name',name,'roster',roster,
         'league',league) ORDER BY league NULLS LAST, name), '[]'::jsonb) FROM teams",
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
    let league = b.league.as_deref().map(str::trim).filter(|s| !s.is_empty());
    match sqlx::query_scalar::<_, i64>(
        "INSERT INTO teams (name, roster, league) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(&b.name)
    .bind(&roster)
    .bind(league)
    .fetch_one(&s.db)
    .await
    {
        Ok(id) => (StatusCode::CREATED, Json(serde_json::json!({"id": id}))),
        Err(e) => db_err(e),
    }
}

#[derive(Deserialize)]
pub struct TeamPatch {
    league: Option<String>,
}

/// PUT /api/teams/{id} (admin) — (re)assigns a team's league.
pub async fn update_team(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(b): Json<TeamPatch>,
) -> (StatusCode, Json<J>) {
    if !is_admin(&headers, &s) {
        return forbidden();
    }
    let league = b.league.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let r = sqlx::query("UPDATE teams SET league = $2 WHERE id = $1")
        .bind(id)
        .bind(league)
        .execute(&s.db)
        .await;
    write_reply(r, "update_team", serde_json::json!({"updated": id}))
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
    let r = sqlx::query("DELETE FROM teams WHERE id = $1")
        .bind(id)
        .execute(&s.db)
        .await;
    write_reply(r, "delete_team", serde_json::json!({"deleted": id}))
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
    let r = sqlx::query("DELETE FROM collections WHERE id = $1")
        .bind(id)
        .execute(&s.db)
        .await;
    write_reply(r, "delete_collection", serde_json::json!({"deleted": id}))
}

/// GET /api/trends — win rate & games aggregated per build (patch proxy), most recent first.
pub async fn trends(State(s): State<AppState>) -> Result<Json<J>, (StatusCode, Json<J>)> {
    // Per HotS build (= patch): played period (first/last_seen), total games, average duration,
    // and the operator perspective (my games + my wins) via app_settings.operator_names.
    let v: J = sqlx::query_scalar(
        "WITH ops AS (
            SELECT lower(jsonb_array_elements_text(value)) AS name
            FROM app_settings WHERE key = 'operator_names'
         )
         SELECT COALESCE(jsonb_agg(t ORDER BY t.last_seen DESC NULLS LAST, t.build DESC), '[]'::jsonb)
         FROM (
            SELECT m.build,
                   count(*) AS games,
                   count(*) FILTER (WHERE m.winner = 0) AS blue_wins,
                   round(avg(m.length)::numeric, 0) AS avg_length,
                   min(m.played_at) AS first_seen,
                   max(m.played_at) AS last_seen,
                   count(mp.win) AS my_games,
                   count(*) FILTER (WHERE mp.win) AS my_wins
            FROM matches m
            LEFT JOIN LATERAL (
                SELECT p.win FROM match_players p
                WHERE p.match_id = m.id AND lower(p.name) IN (SELECT name FROM ops)
                LIMIT 1
            ) mp ON true
            WHERE m.build IS NOT NULL
            GROUP BY m.build
         ) t",
    )
    .fetch_one(&s.db)
    .await
    .map_err(db_err)?;
    Ok(Json(v))
}

/// Response of a simple write: `200 ok` if the database answered (deleting a missing row stays
/// idempotent), `500 {error: db}` otherwise — never a success when the write failed.
fn write_reply(
    r: Result<sqlx::postgres::PgQueryResult, sqlx::Error>,
    what: &str,
    ok: J,
) -> (StatusCode, Json<J>) {
    match r {
        Ok(_) => (StatusCode::OK, Json(ok)),
        Err(e) => {
            tracing::error!("{what}: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "db"})),
            )
        }
    }
}
