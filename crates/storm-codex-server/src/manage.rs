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
    // Mode ouvert : pas de token configuré → écritures autorisées (auto-hébergement local).
    let Some(token) = s.cfg.admin_token.as_deref() else { return true };
    h.get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        == Some(token)
}
fn forbidden() -> (StatusCode, Json<J>) {
    (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "admin token requis"})))
}
fn db_err(e: sqlx::Error) -> (StatusCode, Json<J>) {
    tracing::error!("manage : {e}");
    (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "db"})))
}

// ── Réglages (operator_names…) ───────────────────────────────────────────────
/// GET /api/settings — réglages publics (lecture). Aujourd'hui : `operator_names`.
pub async fn get_settings(State(s): State<AppState>) -> Result<Json<J>, (StatusCode, Json<J>)> {
    let mut v: J = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_object_agg(key, value), '{}'::jsonb) FROM app_settings",
    )
    .fetch_one(&s.db)
    .await
    .map_err(db_err)?;
    // `admin_open` : true = aucune auth admin requise (ADMIN_TOKEN absent) → le front masque le
    // champ token et envoie ses écritures sans Bearer.
    if let Some(obj) = v.as_object_mut() {
        obj.insert("admin_open".into(), J::Bool(s.cfg.admin_token.is_none()));
    }
    Ok(Json(v))
}

#[derive(Deserialize)]
pub struct SettingsPatch {
    operator_names: Vec<String>,
}

/// PUT /api/admin/settings (admin) — met à jour la liste des noms opérateur.
pub async fn put_settings(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(b): Json<SettingsPatch>,
) -> (StatusCode, Json<J>) {
    if !is_admin(&headers, &s) {
        return forbidden();
    }
    // normalise : trim, non vides, dédup en préservant l'ordre
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
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({"operator_names": names}))),
        Err(e) => {
            let (c, j) = db_err(e);
            (c, j)
        }
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

/// GET /api/teams — inclut la ligue (regroupement).
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

/// PUT /api/teams/{id} (admin) — (ré)assigne la ligue d'une équipe.
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
    let _ = sqlx::query("UPDATE teams SET league = $2 WHERE id = $1")
        .bind(id)
        .bind(league)
        .execute(&s.db)
        .await;
    (StatusCode::OK, Json(serde_json::json!({"updated": id})))
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
    // Par build HotS (= patch) : période jouée (first/last_seen), total parties, durée moyenne,
    // et la perspective opérateur (mes parties + mes victoires) via app_settings.operator_names.
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
