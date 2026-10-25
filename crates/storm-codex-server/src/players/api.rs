//! Routes `/api/players/*` de la recherche et de l'identité (spec recherche-joueurs).

use super::search;
use crate::AppState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value as J};

type Err = (StatusCode, Json<J>);
type Resp = Result<Json<J>, Err>;

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
