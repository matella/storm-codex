//! Endpoints REST de lecture (l'API que le front consommera au jalon 4). Postgres construit
//! le JSON (jsonb_agg / jsonb_build_object) ; le handler ne fait que le relayer.

use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
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
    /// compte opérateur précis (sinon : n'importe lequel des operator_names)
    account: Option<String>,
    /// "win" | "loss" — perspective opérateur
    result: Option<String>,
    /// true = uniquement les parties où l'opérateur fut MVP
    #[serde(default)]
    mvp: bool,
    /// true = restreint aux lignes/parties de l'opérateur (operator_names) — agrégats Heroes/Maps
    #[serde(default)]
    mine: bool,
    /// plage de dates (ISO) sur played_at : `from` inclus, `to` exclu
    from: Option<String>,
    to: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    offset: Option<i64>,
}

/// GET /api/matches — liste filtrable, paginée, récents d'abord. Filtres : carte, mode, héros (tout
/// joueur), joueur (toon), + perspective opérateur (compte, résultat V/D, MVP, plage de dates).
pub async fn list_matches(State(s): State<AppState>, Query(f): Query<MatchFilter>) -> Resp {
    let limit = f.limit.unwrap_or(50).clamp(1, 200);
    let offset = f.offset.unwrap_or(0).max(0);
    let v: J = sqlx::query_scalar(
        "WITH ops AS (
            SELECT lower(jsonb_array_elements_text(value)) AS name
            FROM app_settings WHERE key = 'operator_names'
         )
         SELECT COALESCE(jsonb_agg(t ORDER BY t.played_at DESC NULLS LAST), '[]'::jsonb)
         FROM (
           SELECT m.id, m.map, m.mode, m.played_at, m.length, m.winner, m.build,
             (SELECT jsonb_agg(jsonb_build_object(
                 'toon', mp.toon_handle, 'name', mp.name, 'hero', mp.hero,
                 'team', mp.team, 'win', mp.win,
                 'kills', mp.kills, 'deaths', mp.deaths, 'takedowns', mp.takedowns,
                 'award', mp.data #>> '{gameStats,awards,0}')
                 ORDER BY mp.team, mp.id)
              FROM match_players mp WHERE mp.match_id = m.id) AS players
           FROM matches m
           -- ligne de l'opérateur dans ce match : compte précis ($7) sinon n'importe quel
           -- operator_name. Sert aux filtres résultat / MVP / compte.
           LEFT JOIN LATERAL (
             SELECT p.win, p.name, (p.data #>> '{gameStats,awards,0}') AS award
             FROM match_players p
             WHERE p.match_id = m.id
               AND (($7::text IS NOT NULL AND lower(p.name) = lower($7))
                 OR ($7::text IS NULL     AND lower(p.name) IN (SELECT name FROM ops)))
             LIMIT 1
           ) me ON true
           WHERE ($1::text IS NULL OR m.map = $1)
             AND ($2::int  IS NULL OR m.mode = $2)
             AND ($3::text IS NULL OR EXISTS (SELECT 1 FROM match_players h
                                              WHERE h.match_id = m.id AND h.hero = $3))
             AND ($4::text IS NULL OR EXISTS (SELECT 1 FROM match_players p
                                              WHERE p.match_id = m.id AND p.toon_handle = $4))
             AND ($7::text  IS NULL OR me.name IS NOT NULL)
             AND ($8::text  IS NULL OR me.win = ($8 = 'win'))
             AND (NOT $9::bool OR me.award = 'EndOfMatchAwardMVPBoolean')
             AND ($10::timestamptz IS NULL OR m.played_at >= $10::timestamptz)
             AND ($11::timestamptz IS NULL OR m.played_at <  $11::timestamptz)
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
    .bind(f.account)
    .bind(f.result)
    .bind(f.mvp)
    .bind(f.from)
    .bind(f.to)
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
pub async fn list_heroes(State(s): State<AppState>, Query(f): Query<MatchFilter>) -> Resp {
    let v: J = sqlx::query_scalar(
        "WITH ops AS (
            SELECT lower(jsonb_array_elements_text(value)) AS name
            FROM app_settings WHERE key = 'operator_names'
         )
         SELECT COALESCE(jsonb_agg(h ORDER BY h.games DESC), '[]'::jsonb) FROM (
            SELECT mp.hero, count(*) AS games, count(*) FILTER (WHERE mp.win) AS wins
            FROM match_players mp JOIN matches m ON m.id = mp.match_id
            WHERE mp.hero IS NOT NULL
              AND ($1::int IS NULL OR m.mode = $1)
              AND ($2::timestamptz IS NULL OR m.played_at >= $2::timestamptz)
              AND ($3::timestamptz IS NULL OR m.played_at <  $3::timestamptz)
              AND (NOT $4::bool OR lower(mp.name) IN (SELECT name FROM ops))
              AND ($5::text IS NULL OR lower(mp.name) = lower($5))
            GROUP BY mp.hero) h",
    )
    .bind(f.mode)
    .bind(f.from)
    .bind(f.to)
    .bind(f.mine)
    .bind(f.account)
    .fetch_one(&s.db)
    .await
    .map_err(db_err)?;
    Ok(Json(v))
}

/// GET /api/hero/{hero} — fiche héros du point de vue opérateur : volume + WR + KDA moyen, WR par
/// carte, et builds de talents les plus joués (avec WR) pour ce héros.
pub async fn hero_detail(State(s): State<AppState>, Path(hero): Path<String>) -> Resp {
    let v: J = sqlx::query_scalar(
        "WITH ops AS (
            SELECT lower(jsonb_array_elements_text(value)) AS name FROM app_settings WHERE key='operator_names'
         ),
         mine AS (
            SELECT mp.win, mp.kills, mp.deaths, mp.takedowns, m.map, mp.data->'talents' AS talents
            FROM match_players mp JOIN matches m ON m.id = mp.match_id
            WHERE mp.hero = $1 AND lower(mp.name) IN (SELECT name FROM ops)
         )
         SELECT jsonb_build_object(
            'hero', $1::text,
            'games', count(*),
            'wins', count(*) FILTER (WHERE win),
            'avg_kills', round(avg(kills)::numeric, 1),
            'avg_deaths', round(avg(deaths)::numeric, 1),
            'avg_takedowns', round(avg(takedowns)::numeric, 1),
            'by_map', (SELECT COALESCE(jsonb_agg(x ORDER BY x.games DESC), '[]'::jsonb) FROM (
                SELECT map, count(*) AS games, count(*) FILTER (WHERE win) AS wins
                FROM mine GROUP BY map) x),
            'builds', (SELECT COALESCE(jsonb_agg(b ORDER BY b.games DESC), '[]'::jsonb) FROM (
                SELECT talents, count(*) AS games, count(*) FILTER (WHERE win) AS wins
                FROM mine WHERE talents IS NOT NULL AND talents <> '{}'::jsonb
                GROUP BY talents ORDER BY count(*) DESC LIMIT 6) b)
         ) FROM mine",
    )
    .bind(hero)
    .fetch_one(&s.db)
    .await
    .map_err(db_err)?;
    Ok(Json(v))
}

/// GET /api/synergies — perspective opérateur : alliés avec qui tu gagnes le plus (≥3 parties
/// ensemble) et héros adverses rencontrés (≥3 fois) avec ton WR contre eux.
pub async fn synergies(State(s): State<AppState>) -> Resp {
    let v: J = sqlx::query_scalar(
        "WITH ops AS (
            SELECT lower(jsonb_array_elements_text(value)) AS name FROM app_settings WHERE key='operator_names'
         ),
         me AS (
            SELECT mp.match_id, mp.team, mp.win FROM match_players mp
            WHERE lower(mp.name) IN (SELECT name FROM ops)
         )
         SELECT jsonb_build_object(
            'teammates', (SELECT COALESCE(jsonb_agg(t ORDER BY t.games DESC), '[]'::jsonb) FROM (
                SELECT tm.name, count(*) AS games, count(*) FILTER (WHERE me.win) AS wins
                FROM me JOIN match_players tm ON tm.match_id = me.match_id AND tm.team = me.team
                WHERE tm.name IS NOT NULL AND lower(tm.name) NOT IN (SELECT name FROM ops)
                GROUP BY tm.name HAVING count(*) >= 3
                ORDER BY count(*) DESC LIMIT 50) t),
            'enemies', (SELECT COALESCE(jsonb_agg(e ORDER BY e.games DESC), '[]'::jsonb) FROM (
                SELECT en.hero, count(*) AS games, count(*) FILTER (WHERE me.win) AS wins
                FROM me JOIN match_players en ON en.match_id = me.match_id AND en.team <> me.team
                WHERE en.hero IS NOT NULL
                GROUP BY en.hero HAVING count(*) >= 3) e)
         )",
    )
    .fetch_one(&s.db)
    .await
    .map_err(db_err)?;
    Ok(Json(v))
}

/// GET /api/dim/heroes — référentiel héros (nom → univers/rôle/icône) pour les anneaux d'univers.
pub async fn dim_heroes(State(s): State<AppState>) -> Resp {
    let v: J = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_object_agg(name, jsonb_build_object(
            'universe', universe, 'role', role, 'icon', data->'icon')), '{}'::jsonb)
         FROM dim_heroes",
    )
    .fetch_one(&s.db)
    .await
    .map_err(db_err)?;
    Ok(Json(v))
}

/// GET /api/dim/talents — référentiel talents (`talentTreeId` → nom/tier/héros/icône) pour
/// afficher les builds dans la fiche de match. Clé = la valeur stockée par le parser.
pub async fn dim_talents(State(s): State<AppState>) -> Resp {
    let v: J = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_object_agg(tree_id, jsonb_build_object(
            'name', name, 'tier', tier, 'hero', hero_id, 'icon', data->'icon')), '{}'::jsonb)
         FROM dim_talents WHERE tree_id IS NOT NULL",
    )
    .fetch_one(&s.db)
    .await
    .map_err(db_err)?;
    Ok(Json(v))
}

/// GET /api/matches.csv — export CSV des matchs (filtres identiques à /api/matches).
pub async fn matches_csv(
    State(s): State<AppState>,
    Query(f): Query<MatchFilter>,
) -> Result<axum::response::Response, (StatusCode, Json<J>)> {
    use std::fmt::Write;
    type Row = (
        i64,
        Option<String>,
        Option<i32>,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<f64>,
        Option<i32>,
        Option<i32>,
    );
    let rows: Vec<Row> = sqlx::query_as(
            "WITH ops AS (
                SELECT lower(jsonb_array_elements_text(value)) AS name
                FROM app_settings WHERE key = 'operator_names'
             )
             SELECT m.id, m.map, m.mode, m.played_at, m.length, m.winner, m.build
             FROM matches m
             LEFT JOIN LATERAL (
               SELECT p.win, p.name, (p.data #>> '{gameStats,awards,0}') AS award
               FROM match_players p
               WHERE p.match_id = m.id
                 AND (($4::text IS NOT NULL AND lower(p.name) = lower($4))
                   OR ($4::text IS NULL     AND lower(p.name) IN (SELECT name FROM ops)))
               LIMIT 1
             ) me ON true
             WHERE ($1::text IS NULL OR m.map = $1)
               AND ($2::int  IS NULL OR m.mode = $2)
               AND ($5::text IS NULL OR EXISTS (SELECT 1 FROM match_players h
                                                WHERE h.match_id = m.id AND h.hero = $5))
               AND ($4::text IS NULL OR me.name IS NOT NULL)
               AND ($6::text IS NULL OR me.win = ($6 = 'win'))
               AND (NOT $7::bool OR me.award = 'EndOfMatchAwardMVPBoolean')
               AND ($8::timestamptz  IS NULL OR m.played_at >= $8::timestamptz)
               AND ($9::timestamptz  IS NULL OR m.played_at <  $9::timestamptz)
             ORDER BY m.played_at DESC NULLS LAST LIMIT $3",
        )
        .bind(f.map)
        .bind(f.mode)
        .bind(f.limit.unwrap_or(5000).clamp(1, 50000))
        .bind(f.account)
        .bind(f.hero)
        .bind(f.result)
        .bind(f.mvp)
        .bind(f.from)
        .bind(f.to)
        .fetch_all(&s.db)
        .await
        .map_err(db_err)?;
    let mut csv = String::from("id,map,mode,played_at,length,winner,build\n");
    for (id, map, mode, played, length, winner, build) in rows {
        let _ = writeln!(
            csv,
            "{id},{},{},{},{},{},{}",
            map.unwrap_or_default().replace(',', " "),
            mode.map(|v| v.to_string()).unwrap_or_default(),
            played.map(|d| d.to_rfc3339()).unwrap_or_default(),
            length.map(|v| format!("{v:.1}")).unwrap_or_default(),
            winner.map(|v| v.to_string()).unwrap_or_default(),
            build.map(|v| v.to_string()).unwrap_or_default(),
        );
    }
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "text/csv; charset=utf-8"),
            (axum::http::header::CONTENT_DISPOSITION, "attachment; filename=\"matches.csv\""),
        ],
        csv,
    )
        .into_response())
}

/// GET /api/maps — parties par carte + winrate équipe bleue.
pub async fn list_maps(State(s): State<AppState>, Query(f): Query<MatchFilter>) -> Resp {
    let v: J = sqlx::query_scalar(
        "WITH ops AS (
            SELECT lower(jsonb_array_elements_text(value)) AS name
            FROM app_settings WHERE key = 'operator_names'
         )
         SELECT COALESCE(jsonb_agg(t ORDER BY t.games DESC), '[]'::jsonb) FROM (
            SELECT m.map, count(*) AS games,
                   count(*) FILTER (WHERE m.winner = 0) AS blue_wins,
                   round(avg(m.length)::numeric, 0) AS avg_length,
                   count(me.win) AS my_games,
                   count(*) FILTER (WHERE me.win) AS my_wins
            FROM matches m
            -- ligne de l'opérateur (compte précis $5 sinon operator_names) : pour le WR perso/carte
            LEFT JOIN LATERAL (
              SELECT p.win FROM match_players p
              WHERE p.match_id = m.id
                AND (($5::text IS NOT NULL AND lower(p.name) = lower($5))
                  OR ($5::text IS NULL     AND lower(p.name) IN (SELECT name FROM ops)))
              LIMIT 1
            ) me ON true
            WHERE m.map IS NOT NULL
              AND ($1::int IS NULL OR m.mode = $1)
              AND ($2::timestamptz IS NULL OR m.played_at >= $2::timestamptz)
              AND ($3::timestamptz IS NULL OR m.played_at <  $3::timestamptz)
              AND (NOT ($4::bool OR $5::text IS NOT NULL) OR me.win IS NOT NULL)
            GROUP BY m.map) t",
    )
    .bind(f.mode)
    .bind(f.from)
    .bind(f.to)
    .bind(f.mine)
    .bind(f.account)
    .fetch_one(&s.db)
    .await
    .map_err(db_err)?;
    Ok(Json(v))
}

/// GET /api/now-playing — proxifie Orpheus (`/api/playback/now` = lecture Spotify LIVE +
/// `/api/auth/status`) pour le widget musique OBS. `/now` reflète ce qui joue réellement sur
/// Spotify (indépendant de l'engine DJ). Best-effort : Orpheus absent/non authentifié →
/// `{authenticated:false}` (le widget affiche « music off »). Évite CORS, garde l'URL côté serveur.
pub async fn now_playing(State(s): State<AppState>) -> Json<J> {
    let Some(base) = s.cfg.orpheus_url.clone() else {
        return Json(serde_json::json!({ "authenticated": false }));
    };
    let base = base.trim_end_matches('/').to_string();
    let res = tokio::task::spawn_blocking(move || -> J {
        let get = |path: &str| -> Option<J> {
            ureq::get(&format!("{base}{path}"))
                .call()
                .ok()?
                .body_mut()
                .read_json()
                .ok()
        };
        let auth = get("/api/auth/status")
            .and_then(|v| v.get("authenticated").and_then(J::as_bool))
            .unwrap_or(false);
        let current = get("/api/playback/now").unwrap_or(J::Null);
        serde_json::json!({ "authenticated": auth, "current": current })
    })
    .await
    .unwrap_or_else(|_| serde_json::json!({ "authenticated": false }));
    Json(res)
}
