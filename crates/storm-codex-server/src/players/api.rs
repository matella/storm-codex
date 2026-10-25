//! Routes `/api/players/*` de la recherche et de l'identité (spec recherche-joueurs).

use super::{identity, names, search};
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value as J};

type Err = (StatusCode, Json<J>);
type Resp = Result<Json<J>, Err>;

fn err(code: StatusCode, msg: &str) -> Err {
    (code, Json(json!({ "error": msg })))
}

fn db_err(e: sqlx::Error) -> Err {
    tracing::error!("players : {e}");
    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "db" })))
}

#[derive(Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    q: String,
    limit: Option<i64>,
}

/// GET /api/players/search?q=&limit= — `{ "hits": [...] }`. Moins de 2 caractères → liste vide.
pub async fn search(State(s): State<AppState>, Query(p): Query<SearchQuery>) -> Resp {
    let hits = search::search(&s.db, &p.q, p.limit.unwrap_or(search::DEFAULT_LIMIT))
        .await
        .map_err(db_err)?;
    Ok(Json(json!({ "hits": hits })))
}

async fn known(db: &sqlx::PgPool, toon: &str) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM players WHERE toon_handle = $1)
             OR EXISTS (SELECT 1 FROM match_players WHERE toon_handle = $1)",
    )
    .bind(toon)
    .fetch_one(db)
    .await
}

/// GET /api/players/{toon}/identity — historique daté des pseudos, comptes liés **devinés**
/// (estimation, jamais un fait) et liens confirmés par l'opérateur.
pub async fn identity(State(s): State<AppState>, Path(toon): Path<String>) -> Resp {
    if !known(&s.db, &toon).await.map_err(db_err)? {
        return Err(err(StatusCode::NOT_FOUND, "joueur inconnu"));
    }
    let history = names::name_history(&s.db, &toon).await.map_err(db_err)?;
    let name = names::current_name(&s.db, &toon).await.map_err(db_err)?;
    let suggestions = identity::suggest(&s.db, &toon).await.map_err(db_err)?;
    let links: Vec<(String, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT CASE WHEN toon_a = $1 THEN toon_b ELSE toon_a END, decided_at FROM player_links
         WHERE (toon_a = $1 OR toon_b = $1) AND status = 'confirmed' ORDER BY decided_at DESC",
    )
    .bind(&toon)
    .fetch_all(&s.db)
    .await
    .map_err(db_err)?;
    let mut confirmed = Vec::with_capacity(links.len());
    for (other, decided_at) in links {
        let other_name = names::current_name(&s.db, &other).await.map_err(db_err)?;
        confirmed.push(json!({ "toon": other, "name": other_name, "decided_at": decided_at }));
    }
    Ok(Json(json!({
        "toon": toon,
        "name": name,
        "names": history,
        "suggestions": suggestions,
        "confirmed": confirmed,
    })))
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum LinkStatus {
    Confirmed,
    Dismissed,
}

#[derive(Deserialize)]
pub struct LinkBody {
    status: LinkStatus,
}

/// PUT /api/players/{toon}/links/{other} 🔒 — confirme (« même personne », une note : aucune
/// fusion de profils) ou écarte une suggestion. Paire normalisée (min, max) : A→B ≡ B→A, idempotent.
pub async fn put_link(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((toon, other)): Path<(String, String)>,
    Json(body): Json<LinkBody>,
) -> Resp {
    if !crate::manage::is_admin(&headers, &s) {
        return Err(err(StatusCode::UNAUTHORIZED, "admin token requis"));
    }
    if toon == other {
        return Err(err(StatusCode::BAD_REQUEST, "un compte ne peut pas être lié à lui-même"));
    }
    for t in [&toon, &other] {
        if !known(&s.db, t).await.map_err(db_err)? {
            return Err(err(StatusCode::NOT_FOUND, "joueur inconnu"));
        }
    }
    let (a, b) = if toon < other { (&toon, &other) } else { (&other, &toon) };
    let status = match body.status {
        LinkStatus::Confirmed => "confirmed",
        LinkStatus::Dismissed => "dismissed",
    };
    sqlx::query(
        "INSERT INTO player_links (toon_a, toon_b, status) VALUES ($1, $2, $3)
         ON CONFLICT (toon_a, toon_b) DO UPDATE SET status = EXCLUDED.status, decided_at = now()",
    )
    .bind(a)
    .bind(b)
    .bind(status)
    .execute(&s.db)
    .await
    .map_err(db_err)?;
    Ok(Json(json!({ "status": status })))
}
