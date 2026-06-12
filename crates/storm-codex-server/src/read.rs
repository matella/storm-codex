//! Endpoints REST de lecture (l'API que le front consommera au jalon 4). Postgres construit
//! le JSON (jsonb_agg / jsonb_build_object) ; le handler ne fait que le relayer.

use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::Value as J;

type Resp = Result<Json<J>, (StatusCode, Json<J>)>;

fn db_err(e: sqlx::Error) -> (StatusCode, Json<J>) {
    tracing::error!("lecture : {e}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": "db"})),
    )
}

#[derive(Deserialize)]
pub struct MatchFilter {
    map: Option<String>,
    mode: Option<i32>,
    hero: Option<String>,
    player: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    offset: Option<i64>,
}

/// GET /api/matches — liste filtrable (carte/mode/héros/joueur), paginée, récents d'abord.
pub async fn list_matches(State(s): State<AppState>, Query(f): Query<MatchFilter>) -> Resp {
    let limit = f.limit.unwrap_or(50).clamp(1, 200);
    let offset = f.offset.unwrap_or(0).max(0);
    let v: J = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(t ORDER BY t.played_at DESC NULLS LAST), '[]'::jsonb)
         FROM (
           SELECT m.id, m.map, m.mode, m.played_at, m.length, m.winner, m.build,
             (SELECT jsonb_agg(jsonb_build_object(
                 'toon', mp.toon_handle, 'name', mp.name, 'hero', mp.hero,
                 'team', mp.team, 'win', mp.win) ORDER BY mp.team, mp.id)
              FROM match_players mp WHERE mp.match_id = m.id) AS players
           FROM matches m
           WHERE ($1::text IS NULL OR m.map = $1)
             AND ($2::int  IS NULL OR m.mode = $2)
             AND ($3::text IS NULL OR EXISTS (SELECT 1 FROM match_players h
                                              WHERE h.match_id = m.id AND h.hero = $3))
             AND ($4::text IS NULL OR EXISTS (SELECT 1 FROM match_players p
                                              WHERE p.match_id = m.id AND p.toon_handle = $4))
           ORDER BY m.played_at DESC NULLS LAST
           LIMIT $5 OFFSET $6
         ) t",
    )
    .bind(f.map)
    .bind(f.mode)
    .bind(f.hero)
    .bind(f.player)
    .bind(limit)
    .bind(offset)
    .fetch_one(&s.db)
    .await
    .map_err(db_err)?;
    Ok(Json(v))
}

/// GET /api/matches/{id} — détail complet (forme `{match, players}` de storm-stats).
pub async fn get_match(State(s): State<AppState>, Path(id): Path<i64>) -> Resp {
    let v: Option<J> = sqlx::query_scalar(
        "SELECT jsonb_build_object(
            'id', m.id, 'fingerprint', m.fingerprint, 'parser_version', m.parser_version,
            'match', m.data,
            'players', (SELECT jsonb_object_agg(mp.toon_handle, mp.data)
                        FROM match_players mp WHERE mp.match_id = m.id))
         FROM matches m WHERE m.id = $1",
    )
    .bind(id)
    .fetch_optional(&s.db)
    .await
    .map_err(db_err)?;
    match v {
        Some(v) => Ok(Json(v)),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "match inconnu"})),
        )),
    }
}

/// GET /api/players/{toon} — résumé joueur + hero pool.
pub async fn get_player(State(s): State<AppState>, Path(toon): Path<String>) -> Resp {
    let v: J = sqlx::query_scalar(
        "SELECT jsonb_build_object(
            'toon', $1::text,
            'name', (SELECT last_name FROM players WHERE toon_handle = $1),
            'names', COALESCE((SELECT names FROM players WHERE toon_handle = $1), '[]'::jsonb),
            'matches', (SELECT count(*) FROM match_players WHERE toon_handle = $1),
            'wins', (SELECT count(*) FROM match_players WHERE toon_handle = $1 AND win),
            'heroes', COALESCE((SELECT jsonb_agg(h ORDER BY h.games DESC) FROM (
                SELECT hero, count(*) AS games, count(*) FILTER (WHERE win) AS wins
                FROM match_players WHERE toon_handle = $1 AND hero IS NOT NULL
                GROUP BY hero) h), '[]'::jsonb))",
    )
    .bind(toon)
    .fetch_one(&s.db)
    .await
    .map_err(db_err)?;
    Ok(Json(v))
}

/// GET /api/heroes — stats agrégées par héros (games/wins).
pub async fn list_heroes(State(s): State<AppState>) -> Resp {
    let v: J = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(h ORDER BY h.games DESC), '[]'::jsonb) FROM (
            SELECT hero, count(*) AS games, count(*) FILTER (WHERE win) AS wins
            FROM match_players WHERE hero IS NOT NULL GROUP BY hero) h",
    )
    .fetch_one(&s.db)
    .await
    .map_err(db_err)?;
    Ok(Json(v))
}

/// GET /api/maps — parties par carte + winrate équipe bleue.
pub async fn list_maps(State(s): State<AppState>) -> Resp {
    let v: J = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(t ORDER BY t.games DESC), '[]'::jsonb) FROM (
            SELECT map, count(*) AS games,
                   count(*) FILTER (WHERE winner = 0) AS blue_wins,
                   round(avg(length)::numeric, 0) AS avg_length
            FROM matches WHERE map IS NOT NULL GROUP BY map) t",
    )
    .fetch_one(&s.db)
    .await
    .map_err(db_err)?;
    Ok(Json(v))
}
