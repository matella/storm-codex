//! Read REST endpoints (the API the front will consume at milestone 4). Postgres builds
//! the JSON (jsonb_agg / jsonb_build_object); the handler only relays it.

use crate::{standalone::operator_toons, AppState};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::Value as J;
use sqlx::{Postgres, QueryBuilder};

type Resp = Result<Json<J>, (StatusCode, Json<J>)>;

mod filter;
#[cfg(test)]
mod tests;

fn db_err(e: sqlx::Error) -> (StatusCode, Json<J>) {
    tracing::error!("read: {e}");
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
    /// specific operator account (otherwise: any of the operator_names)
    account: Option<String>,
    /// "win" | "loss" — operator perspective
    result: Option<String>,
    /// true = only the games where the operator was MVP
    #[serde(default)]
    mvp: bool,
    /// true = restricted to the operator's rows/games (operator_names) — Heroes/Maps aggregates
    #[serde(default)]
    mine: bool,
    /// date range (ISO) on played_at: `from` included, `to` excluded
    from: Option<String>,
    to: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    offset: Option<i64>,
}

/// GET /api/matches — filterable, paginated list, most recent first. Filters: map, mode, hero (any
/// player), player (toon), + operator perspective (account, W/L result, MVP, date range) and the replay
/// search filters (`read/filter.rs`). The body is a bare array; `X-Total-Count` carries the number of
/// games matching the filters (ignoring `limit`/`offset`).
pub async fn list_matches(
    State(s): State<AppState>,
    Query(f): Query<MatchFilter>,
    Query(raw): Query<Vec<(String, String)>>,
) -> Result<axum::response::Response, (StatusCode, Json<J>)> {
    let search = archive_search(&s, &f, &raw).await?;
    let limit = f.limit.unwrap_or(50).clamp(1, 200);
    let offset = f.offset.unwrap_or(0).max(0);
    let toons = operator_toons(&s);
    // The page is chosen first (ids only), then only its rows are expanded: deep offsets stay cheap.
    let mut list = QueryBuilder::<Postgres>::new(
        "WITH ops AS (
            SELECT lower(jsonb_array_elements_text(value)) AS name
            FROM app_settings WHERE key = 'operator_names'
         )
         SELECT COALESCE(jsonb_agg(t ORDER BY t.played_at DESC NULLS LAST, t.id DESC), '[]'::jsonb)
         FROM (
           SELECT m.id, m.map, m.mode, m.played_at, m.length, m.winner, m.build,
             me.toon_handle AS perspective_toon,
             (SELECT jsonb_agg(jsonb_build_object(
                 'toon', mp.toon_handle, 'name', mp.name, 'hero', mp.hero,
                 'team', mp.team, 'win', mp.win,
                 'kills', mp.kills, 'deaths', mp.deaths, 'takedowns', mp.takedowns,
                 'award', mp.data #>> '{gameStats,awards,0}')
                 ORDER BY mp.team, mp.id)
              FROM match_players mp WHERE mp.match_id = m.id) AS players
           FROM (SELECT m.id",
    );
    push_archive_from(&mut list, &f, &search, &toons);
    list.push(" ORDER BY m.played_at DESC NULLS LAST, m.id DESC LIMIT ")
        .push_bind(limit)
        .push(" OFFSET ")
        .push_bind(offset)
        .push(") page JOIN matches m ON m.id = page.id");
    push_me_join(&mut list, f.account.as_deref(), &toons, true);
    list.push(") t");
    let mut count = QueryBuilder::<Postgres>::new(
        "WITH ops AS (
            SELECT lower(jsonb_array_elements_text(value)) AS name
            FROM app_settings WHERE key = 'operator_names'
         )
         SELECT count(*)",
    );
    push_archive_from(&mut count, &f, &search, &toons);
    let (v, total): (J, i64) = tokio::try_join!(
        list.build_query_scalar().fetch_one(&s.db),
        count.build_query_scalar().fetch_one(&s.db),
    )
    .map_err(db_err)?;
    Ok(([(TOTAL_COUNT, total.to_string())], Json(v)).into_response())
}

/// Response header of `/api/matches` with the number of games matching the filters.
const TOTAL_COUNT: &str = "x-total-count";

/// Parses the replay search filters and checks they have a perspective to apply to.
async fn archive_search(
    s: &AppState,
    f: &MatchFilter,
    raw: &[(String, String)],
) -> Result<filter::Search, (StatusCode, Json<J>)> {
    let search = filter::parse(raw).map_err(|filter::BadFilter(param)| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "bad_filter", "param": param})),
        )
    })?;
    if search.needs_perspective()
        && f.account.as_deref().is_none_or(str::is_empty)
        && operator_toons(s).is_empty()
    {
        let names: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM app_settings
               WHERE key = 'operator_names' AND jsonb_typeof(value) = 'array'
                 AND jsonb_array_length(value) > 0)",
        )
        .fetch_one(&s.db)
        .await
        .map_err(db_err)?;
        if !names {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "perspective_required"})),
            ));
        }
    }
    Ok(search)
}

/// The operator perspective row `me` of game `m`: verified toons first, then manual `operator_names`;
/// an explicit `account` restricts it (`toon:` accounts only match verified toons). One definition for the
/// archive, its filters, its count and the CSV export. `per_game` = a lateral lookup per row of `m`
/// (cheap for one page); otherwise one pass over all perspective rows, joined on `match_id` (cheaper
/// than thousands of lookups when a filter reads `me` on every game).
fn push_me_join(
    qb: &mut QueryBuilder<Postgres>,
    account: Option<&str>,
    toons: &[String],
    per_game: bool,
) {
    qb.push(if per_game {
        " LEFT JOIN LATERAL (SELECT"
    } else {
        " LEFT JOIN (SELECT DISTINCT ON (p.match_id) p.match_id,"
    });
    qb.push(
        // No JSON here: reading `data` would detoast it for every game.
        " p.win, p.toon_handle, p.team, p.hero, p.kills, p.deaths, p.takedowns,
                  p.hero_damage, p.healing, p.experience, p.apm
           FROM match_players p WHERE ",
    );
    if per_game {
        qb.push("p.match_id = m.id AND ");
    }
    match account {
        None => {
            qb.push("(lower(p.name) IN (SELECT name FROM ops) OR p.toon_handle = ANY(")
                .push_bind(toons.to_vec())
                .push("))");
        }
        Some(a) if a.starts_with("toon:") => {
            qb.push("p.toon_handle = ANY(")
                .push_bind(toons.to_vec())
                .push(") AND 'toon:' || p.toon_handle = ")
                .push_bind(a.to_string());
        }
        Some(a) => {
            qb.push("lower(p.name) = lower(")
                .push_bind(a.to_string())
                .push(")");
        }
    }
    qb.push(if per_game {
        " ORDER BY (p.toon_handle = ANY("
    } else {
        " ORDER BY p.match_id, (p.toon_handle = ANY("
    })
    .push_bind(toons.to_vec())
    .push(if per_game {
        ")) DESC, p.team, p.id LIMIT 1) me ON true"
    } else {
        ")) DESC, p.team, p.id) me ON me.match_id = m.id"
    });
}

/// `FROM matches m` and the WHERE clause shared by the archive list, its count and the CSV export. The
/// perspective row is only joined when a filter reads it.
fn push_archive_from(
    qb: &mut QueryBuilder<Postgres>,
    f: &MatchFilter,
    search: &filter::Search,
    toons: &[String],
) {
    let account = f.account.as_deref();
    let needs_me = account.is_some() || f.result.is_some() || f.mvp || search.needs_perspective();
    qb.push(" FROM matches m");
    if needs_me {
        push_me_join(qb, account, toons, false);
    }
    qb.push(" WHERE true");
    if let Some(map) = &f.map {
        qb.push(" AND m.map = ").push_bind(map.clone());
    }
    if let Some(mode) = f.mode {
        qb.push(" AND m.mode = ").push_bind(mode);
    }
    if let Some(hero) = &f.hero {
        qb.push(" AND EXISTS (SELECT 1 FROM match_players h WHERE h.match_id = m.id AND h.hero = ")
            .push_bind(hero.clone())
            .push(")");
    }
    if let Some(player) = &f.player {
        qb.push(
            " AND EXISTS (SELECT 1 FROM match_players pl WHERE pl.match_id = m.id AND pl.toon_handle = ",
        )
        .push_bind(player.clone())
        .push(")");
    }
    if account.is_some() {
        qb.push(" AND me.toon_handle IS NOT NULL");
    }
    if let Some(result) = &f.result {
        qb.push(" AND me.win = (")
            .push_bind(result.clone())
            .push(" = 'win')");
    }
    if f.mvp {
        // Same row as `me` (UNIQUE (match_id, toon_handle)); served by the partial index of migration 0018.
        qb.push(
            " AND EXISTS (SELECT 1 FROM match_players a WHERE a.match_id = m.id
               AND a.toon_handle = me.toon_handle
               AND (a.data #>> '{gameStats,awards,0}') = 'EndOfMatchAwardMVPBoolean')",
        );
    }
    if let Some(from) = &f.from {
        qb.push(" AND m.played_at >= ")
            .push_bind(from.clone())
            .push("::timestamptz");
    }
    if let Some(to) = &f.to {
        qb.push(" AND m.played_at < ")
            .push_bind(to.clone())
            .push("::timestamptz");
    }
    search.push_where(qb);
}

/// GET /api/matches/{id} — complete detail (storm-stats `{match, players}` shape).
pub async fn get_match(State(s): State<AppState>, Path(id): Path<i64>) -> Resp {
    let v: Option<J> = sqlx::query_scalar(
        "SELECT jsonb_build_object(
            'id', m.id, 'fingerprint', m.fingerprint, 'parser_version', m.parser_version,
            'match', m.data,
            'players', (SELECT jsonb_object_agg(mp.toon_handle,
                            mp.data || jsonb_build_object('apm', round(mp.apm::numeric, 1)))
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

/// GET /api/players/{toon} — player summary + hero pool.
pub async fn get_player(State(s): State<AppState>, Path(toon): Path<String>) -> Resp {
    // Current nickname and nickname list come from the dated history (most recent first), not from
    // `players.last_name`/`names` (processing order). Fallback on the old behaviour if empty.
    let history = crate::players::names::name_history(&s.db, &toon)
        .await
        .map_err(db_err)?;
    let mut v: J = sqlx::query_scalar(
        "SELECT jsonb_build_object(
            'toon', $1::text,
            'name', (SELECT last_name FROM players WHERE toon_handle = $1),
            'names', COALESCE((SELECT names FROM players WHERE toon_handle = $1), '[]'::jsonb),
            'matches', (SELECT count(*) FROM match_players WHERE toon_handle = $1),
            'wins', (SELECT count(*) FROM match_players WHERE toon_handle = $1 AND win),
            'avg_kills', (SELECT round(avg(kills)::numeric, 1) FROM match_players WHERE toon_handle = $1),
            'avg_deaths', (SELECT round(avg(deaths)::numeric, 1) FROM match_players WHERE toon_handle = $1),
            'avg_takedowns', (SELECT round(avg(takedowns)::numeric, 1) FROM match_players WHERE toon_handle = $1),
            'avg_apm', (SELECT round(avg(apm)::numeric, 0) FROM match_players WHERE toon_handle = $1),
            'apm_games', (SELECT count(apm) FROM match_players WHERE toon_handle = $1),
            'heroes', COALESCE((SELECT jsonb_agg(h ORDER BY h.games DESC) FROM (
                SELECT hero, count(*) AS games, count(*) FILTER (WHERE win) AS wins,
                       round(avg(kills)::numeric, 1) AS avg_kills,
                       round(avg(deaths)::numeric, 1) AS avg_deaths,
                       round(avg(takedowns)::numeric, 1) AS avg_takedowns
                FROM match_players WHERE toon_handle = $1 AND hero IS NOT NULL
                GROUP BY hero) h), '[]'::jsonb),
            'recent', COALESCE((SELECT jsonb_agg(r ORDER BY r.played_at DESC NULLS LAST) FROM (
                SELECT mp.match_id, mp.hero, m.map, m.mode, mp.win, mp.kills, mp.deaths, mp.takedowns,
                       round(mp.apm::numeric, 0) AS apm,
                       m.played_at, mp.data #>> '{gameStats,awards,0}' AS award
                FROM match_players mp JOIN matches m ON m.id = mp.match_id
                WHERE mp.toon_handle = $1 ORDER BY m.played_at DESC NULLS LAST LIMIT 20) r), '[]'::jsonb))",
    )
    .bind(&toon)
    .fetch_one(&s.db)
    .await
    .map_err(db_err)?;
    if let Some(cur) = history.first() {
        v["name"] = J::from(cur.name.as_str());
        v["names"] = history.iter().map(|n| J::from(n.name.as_str())).collect();
    }
    Ok(Json(v))
}

/// GET /api/heroes — aggregated stats per hero (games/wins).
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
              AND (NOT $4::bool OR (lower(mp.name) IN (SELECT name FROM ops) OR mp.toon_handle = ANY($6::text[])))
              AND ($5::text IS NULL OR (lower(mp.name) = lower($5) OR (mp.toon_handle = ANY($6::text[]) AND 'toon:' || mp.toon_handle = $5)))
            GROUP BY mp.hero) h",
    )
    .bind(f.mode)
    .bind(f.from)
    .bind(f.to)
    .bind(f.mine)
    .bind(f.account)
    .bind(operator_toons(&s))
    .fetch_one(&s.db)
    .await
    .map_err(db_err)?;
    Ok(Json(v))
}

/// GET /api/hero/{hero} — hero sheet from the operator's point of view: volume + WR + average KDA, WR per
/// map, and most played talent builds (with WR) for this hero.
pub async fn hero_detail(
    State(s): State<AppState>,
    Path(hero): Path<String>,
    Query(f): Query<MatchFilter>,
) -> Resp {
    // Same filters as /api/heroes (mode/from/to/mine/account) → list ↔ sheet consistency.
    // Default (mine=false, no account) = all players, like the list.
    let v: J = sqlx::query_scalar(
        "WITH ops AS (
            SELECT lower(jsonb_array_elements_text(value)) AS name FROM app_settings WHERE key='operator_names'
         ),
         g AS (
            SELECT mp.win, mp.kills, mp.deaths, mp.takedowns, m.map, mp.data->'talents' AS talents
            FROM match_players mp JOIN matches m ON m.id = mp.match_id
            WHERE mp.hero = $1
              AND ($2::int IS NULL OR m.mode = $2)
              AND ($3::timestamptz IS NULL OR m.played_at >= $3::timestamptz)
              AND ($4::timestamptz IS NULL OR m.played_at <  $4::timestamptz)
              AND (NOT $5::bool OR (lower(mp.name) IN (SELECT name FROM ops) OR mp.toon_handle = ANY($7::text[])))
              AND ($6::text IS NULL OR (lower(mp.name) = lower($6) OR (mp.toon_handle = ANY($7::text[]) AND 'toon:' || mp.toon_handle = $6)))
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
                FROM g GROUP BY map) x),
            'builds', (SELECT COALESCE(jsonb_agg(b ORDER BY b.games DESC), '[]'::jsonb) FROM (
                SELECT talents, count(*) AS games, count(*) FILTER (WHERE win) AS wins
                FROM g WHERE talents IS NOT NULL AND talents <> '{}'::jsonb
                GROUP BY talents ORDER BY count(*) DESC LIMIT 6) b)
         ) FROM g",
    )
    .bind(hero)
    .bind(f.mode)
    .bind(f.from)
    .bind(f.to)
    .bind(f.mine)
    .bind(f.account)
    .bind(operator_toons(&s))
    .fetch_one(&s.db)
    .await
    .map_err(db_err)?;
    Ok(Json(v))
}

/// GET /api/synergies — operator perspective: allies you win the most with (≥3 games
/// together) and enemy heroes met (≥3 times) with your WR against them.
pub async fn synergies(State(s): State<AppState>) -> Resp {
    let v: J = sqlx::query_scalar(
        "WITH ops AS (
            SELECT lower(jsonb_array_elements_text(value)) AS name FROM app_settings WHERE key='operator_names'
         ),
         me AS (
            SELECT mp.match_id, mp.team, mp.win FROM match_players mp
            WHERE (lower(mp.name) IN (SELECT name FROM ops) OR mp.toon_handle = ANY($1::text[]))
         )
         SELECT jsonb_build_object(
            'teammates', (SELECT COALESCE(jsonb_agg(t ORDER BY t.games DESC), '[]'::jsonb) FROM (
                SELECT tm.name, count(*) AS games, count(*) FILTER (WHERE me.win) AS wins
                FROM me JOIN match_players tm ON tm.match_id = me.match_id AND tm.team = me.team
                WHERE tm.name IS NOT NULL AND lower(tm.name) NOT IN (SELECT name FROM ops) AND NOT (tm.toon_handle = ANY($1::text[]))
                GROUP BY tm.name HAVING count(*) >= 3
                ORDER BY count(*) DESC LIMIT 50) t),
            'enemies', (SELECT COALESCE(jsonb_agg(e ORDER BY e.games DESC), '[]'::jsonb) FROM (
                SELECT en.hero, count(*) AS games, count(*) FILTER (WHERE me.win) AS wins
                FROM me JOIN match_players en ON en.match_id = me.match_id AND en.team <> me.team
                WHERE en.hero IS NOT NULL
                GROUP BY en.hero HAVING count(*) >= 3) e)
         )",
    )
    .bind(operator_toons(&s))
    .fetch_one(&s.db)
    .await
    .map_err(db_err)?;
    Ok(Json(v))
}

/// GET /api/dim/heroes — hero reference data (name → universe/role/icon) for the universe rings.
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

/// GET /api/dim/hero-attributes — hero attribute code (4 letters) → canonical parser name
/// (`heroAttribute` of `attr.json`, frozen in storm-stats). Used to resolve the draft bans,
/// which storm-stats stores raw (`match.bans[t][i].hero` = "Crus", "DEAT"…). Static, without a DB:
/// `dim_heroes` does not carry the `attributeId` (missing from the HotsPatchNotes list).
pub async fn dim_hero_attributes() -> Json<J> {
    Json(storm_stats::constants::hero_attribute().clone())
}

/// GET /api/dim/talents — talent reference data (`talentTreeId` → name/tier/hero/icon) to
/// show the builds in the match sheet. Key = the value stored by the parser.
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

/// GET /api/matches.csv — CSV export of the matches (same filters, perspective and paging as /api/matches;
/// `limit` default 5,000, max 50,000).
pub async fn matches_csv(
    State(s): State<AppState>,
    Query(f): Query<MatchFilter>,
    Query(raw): Query<Vec<(String, String)>>,
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
    let search = archive_search(&s, &f, &raw).await?;
    let mut qb = QueryBuilder::<Postgres>::new(
        "WITH ops AS (
            SELECT lower(jsonb_array_elements_text(value)) AS name
            FROM app_settings WHERE key = 'operator_names'
         )
         SELECT m.id, m.map, m.mode, m.played_at, m.length, m.winner, m.build",
    );
    push_archive_from(&mut qb, &f, &search, &operator_toons(&s));
    qb.push(" ORDER BY m.played_at DESC NULLS LAST, m.id DESC LIMIT ")
        .push_bind(f.limit.unwrap_or(5000).clamp(1, 50000))
        .push(" OFFSET ")
        .push_bind(f.offset.unwrap_or(0).max(0));
    let rows: Vec<Row> = qb.build_query_as().fetch_all(&s.db).await.map_err(db_err)?;
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
            (
                axum::http::header::CONTENT_DISPOSITION,
                "attachment; filename=\"matches.csv\"",
            ),
        ],
        csv,
    )
        .into_response())
}

/// GET /api/maps — games per map + blue team win rate.
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
            -- operator's row (specific account $5 otherwise operator_names): for the personal/map WR
            LEFT JOIN LATERAL (
              SELECT p.win FROM match_players p
              WHERE p.match_id = m.id
                AND (($5::text IS NOT NULL AND (lower(p.name) = lower($5) OR (p.toon_handle = ANY($6::text[]) AND 'toon:' || p.toon_handle = $5)))
                  OR ($5::text IS NULL     AND (lower(p.name) IN (SELECT name FROM ops) OR p.toon_handle = ANY($6::text[]))))
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
    .bind(operator_toons(&s))
    .fetch_one(&s.db)
    .await
    .map_err(db_err)?;
    Ok(Json(v))
}

/// GET /api/patches — list of the patch notes from `dim_patches` (owned by storm-codex; written by
/// the `refdata` adapter). `{items:[…]}` shape (front compat); `channel` and `communityOnly` added.
pub async fn patches_list(State(s): State<AppState>) -> Json<J> {
    let v: J = sqlx::query_scalar(
        "SELECT jsonb_build_object('items', COALESCE(jsonb_agg(jsonb_build_object(
            'internalId', internal_id, 'patchName', name, 'patchType', type, 'liveDate', live_date,
            'heroCount', hero_count, 'mapCount', map_count,
            'officialLink', data->>'officialLink', 'hasContent', data->'hasContent',
            'channel', data->'channel', 'communityOnly', data->'communityOnly'
         ) ORDER BY live_date DESC NULLS LAST), '[]'::jsonb))
         FROM dim_patches",
    )
    .fetch_one(&s.db)
    .await
    .unwrap_or_else(|_| serde_json::json!({ "items": [] }));
    Json(v)
}

/// GET /api/patches/{id} — detail of a patch, stored offline in `dim_patches.data.detail` (old API
/// shape, rebuilt by `refdata`). `id` is the dataset id (`live:<build>`, `source:…`) or one of its
/// `aliases` (the old HotsPatchNotes `internalId`): an alias answers with the canonical patch, whose
/// `internalId` is the dataset id. `null` when unknown.
pub async fn patch_detail(State(s): State<AppState>, Path(id): Path<String>) -> Json<J> {
    let detail: Option<J> = sqlx::query_scalar(
        "SELECT data->'detail' FROM dim_patches WHERE internal_id = $1 OR $1 = ANY(aliases)
         ORDER BY (internal_id = $1) DESC LIMIT 1",
    )
    .bind(&id)
    .fetch_optional(&s.db)
    .await
    .ok()
    .flatten();
    Json(detail.unwrap_or(J::Null))
}

#[derive(Deserialize)]
pub struct HeroChangesFilter {
    hero: Option<String>,
    class: Option<String>, // BUFF | NERF | REWORK | MIXED | NEW
    patch: Option<String>, // patch_internal_id
    limit: Option<i64>,
    offset: Option<i64>,
}

/// GET /api/hero-changes — global feed of the hero adjustments (all Hero-type sections), most recent
/// first, filterable by hero / classification / patch + paginated. Feeds the "by hero" view. PTR
/// candidates (`channel = 'ptr'`) are not live changes and stay out of it.
pub async fn hero_changes(
    State(s): State<AppState>,
    Query(f): Query<HeroChangesFilter>,
) -> Json<J> {
    let limit = f.limit.unwrap_or(60).clamp(1, 300);
    let offset = f.offset.unwrap_or(0).max(0);
    let hkey = f
        .hero
        .as_deref()
        .filter(|h| !h.is_empty())
        .map(crate::refdata::hero_key);
    let v: J = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(jsonb_build_object(
            'patchInternalId', patch_internal_id, 'patchName', patch_name, 'patchType', patch_type,
            'liveDate', live_date, 'anchor', anchor, 'heroName', hero_name,
            'classification', classification, 'shortSummary', short_summary, 'content', content
         ) ORDER BY live_date DESC NULLS LAST, hero_name), '[]'::jsonb)
         FROM (
            SELECT * FROM patch_hero_sections
            WHERE channel IS DISTINCT FROM 'ptr'
              AND ($1::text IS NULL OR hero_key = $1)
              AND ($2::text IS NULL OR upper(classification) = upper($2))
              AND ($3::text IS NULL OR patch_internal_id = $3)
            ORDER BY live_date DESC NULLS LAST, hero_name
            LIMIT $4 OFFSET $5
         ) ph",
    )
    .bind(hkey)
    .bind(f.class.filter(|c| !c.is_empty()))
    .bind(f.patch.filter(|p| !p.is_empty()))
    .bind(limit)
    .bind(offset)
    .fetch_one(&s.db)
    .await
    .unwrap_or_else(|_| serde_json::json!([]));
    Json(v)
}

/// GET /api/hero-changes/heroes — one hero = one row: number of changes, last patch/date/
/// classification. For the (grouped) "by hero" view. Filterable by classification / patch.
pub async fn hero_changes_heroes(
    State(s): State<AppState>,
    Query(f): Query<HeroChangesFilter>,
) -> Json<J> {
    let v: J = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(jsonb_build_object(
            'heroName', hero_name, 'count', cnt, 'latestDate', latest,
            'latestClass', latest_class, 'latestPatch', latest_patch
         ) ORDER BY latest DESC NULLS LAST, hero_name), '[]'::jsonb)
         FROM (
            SELECT hero_name, count(*) AS cnt, max(live_date) AS latest,
                   (array_agg(classification ORDER BY live_date DESC NULLS LAST))[1] AS latest_class,
                   (array_agg(patch_name    ORDER BY live_date DESC NULLS LAST))[1] AS latest_patch
            FROM patch_hero_sections
            WHERE channel IS DISTINCT FROM 'ptr'
              AND ($1::text IS NULL OR upper(classification) = upper($1))
              AND ($2::text IS NULL OR patch_internal_id = $2)
            GROUP BY hero_name
         ) h",
    )
    .bind(f.class.filter(|c| !c.is_empty()))
    .bind(f.patch.filter(|p| !p.is_empty()))
    .fetch_one(&s.db)
    .await
    .unwrap_or_else(|_| serde_json::json!([]));
    Json(v)
}

/// GET /api/hero/{hero}/patches — patch note sections concerning this hero (hero → patch direction),
/// most recent first. Tolerant join through the normalised key (see `refdata::hero_key`).
pub async fn hero_patches(State(s): State<AppState>, Path(hero): Path<String>) -> Json<J> {
    let key = crate::refdata::hero_key(&hero);
    let v: J = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(jsonb_build_object(
            'patchInternalId', patch_internal_id, 'patchName', patch_name, 'patchType', patch_type,
            'liveDate', live_date, 'anchor', anchor, 'heroName', hero_name,
            'classification', classification, 'shortSummary', short_summary, 'content', content
         ) ORDER BY live_date DESC NULLS LAST), '[]'::jsonb)
         FROM patch_hero_sections WHERE hero_key = $1",
    )
    .bind(&key)
    .fetch_one(&s.db)
    .await
    .unwrap_or_else(|_| serde_json::json!([]));
    Json(v)
}

/// GET /api/now-playing — proxies Orpheus (`/api/playback/now` = LIVE Spotify playback +
/// `/api/auth/status`) for the OBS music widget. `/now` reflects what really plays on
/// Spotify (independent of the DJ engine). Best-effort: Orpheus missing/not authenticated →
/// `{authenticated:false}` (the widget shows "music off"). Avoids CORS, keeps the URL on the server side.
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
