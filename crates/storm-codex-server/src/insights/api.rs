//! Local insights endpoints (spec `docs/specs/2026-11-17-local-insights-design.md`): match fights / camps /
//! time window, records, early leads, head to head, personal insights. Read-only, computed from the stored
//! storm-stats JSON on each request.

use super::cache::{self, Corpus};
use super::logic::{self, MatchFacts};
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value as J};
use std::collections::HashMap;

type Err = (StatusCode, Json<J>);
type Resp = Result<Json<J>, Err>;

fn db_err(e: sqlx::Error) -> Err {
    tracing::error!("insights: {e}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": "db" })),
    )
}

fn bad(msg: &str) -> Err {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": msg })))
}

/// Operator accounts (`operator_names`, lower case), shared CTE of the queries below.
const OPS: &str = "ops AS (SELECT lower(jsonb_array_elements_text(value)) AS name
                   FROM app_settings WHERE key = 'operator_names')";

// ── F3 / F4 / F5: one match ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct WindowQuery {
    from: Option<f64>,
    to: Option<f64>,
}

/// GET /api/matches/{id}/insights?from=&to= — team fights, camps with their reason, isolated deaths, the
/// whole-game teamfight totals per player, and the time window when both bounds are given.
pub async fn match_insights(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<WindowQuery>,
) -> Resp {
    let data: Option<J> = sqlx::query_scalar("SELECT data FROM matches WHERE id = $1")
        .bind(id)
        .fetch_optional(&s.db)
        .await
        .map_err(db_err)?;
    let Some(data) = data else {
        return Err((StatusCode::NOT_FOUND, Json(json!({ "error": "not_found" }))));
    };
    let rows: Vec<(String, Option<i32>, Option<J>)> = sqlx::query_as(
        "SELECT toon_handle, team, data->'gameStats' FROM match_players WHERE match_id = $1 ORDER BY team, id",
    )
    .bind(id)
    .fetch_all(&s.db)
    .await
    .map_err(db_err)?;
    let teams: HashMap<String, u8> = rows
        .iter()
        .filter_map(|(t, team, _)| Some((t.clone(), cache::team(*team)?)))
        .collect();
    let facts = MatchFacts::from_match(&data, &teams);
    let fights = logic::team_fights(&facts);
    let camps = logic::camp_reasons(&facts, &fights);
    let isolated = logic::isolated_deaths(&facts, &fights);
    let toons: Vec<String> = rows.iter().map(|(t, _, _)| t.clone()).collect();
    let window = match (q.from, q.to) {
        (Some(a), Some(b)) if a.is_finite() && b.is_finite() => {
            Some(logic::window(&facts, &toons, a, b))
        }
        (None, None) => None,
        _ => return Err(bad("from_and_to")),
    };
    let teamfight: Vec<J> = rows
        .iter()
        .map(|(toon, _, gs)| {
            let g = |k: &str| gs.as_ref().and_then(|g| g[k].as_f64());
            json!({
                "toon": toon,
                "hero_damage": g("TeamfightHeroDamage"),
                "damage_taken": g("TeamfightDamageTaken"),
                "healing": g("TeamfightHealingDone"),
            })
        })
        .collect();
    Ok(Json(json!({
        "fights": fights,
        "camps": camps,
        "isolated_deaths": isolated,
        "window": window,
        "teamfight_totals": teamfight,
    })))
}

// ── F1: records ──────────────────────────────────────────────────────────────

/// (key, label), in the order of `cache::PlayerRow::stats`.
pub const RECORD_STATS: [(&str, &str); cache::N_STATS] = [
    ("kills", "Most kills"),
    ("takedowns", "Most takedowns"),
    ("assists", "Most assists"),
    ("deaths", "Most deaths"),
    ("hero_damage", "Most hero damage"),
    ("siege_damage", "Most siege damage"),
    ("healing", "Most healing"),
    ("self_healing", "Most self-healing"),
    ("damage_taken", "Most damage taken"),
    ("experience", "Most XP"),
    ("camps", "Most camps"),
    ("cc", "Most time CC'ing heroes"),
];

#[derive(Deserialize)]
pub struct RecordsQuery {
    mode: Option<i32>,
    map: Option<String>,
    hero: Option<String>,
    #[serde(default)]
    mine: bool,
    limit: Option<i64>,
}

/// Operator accounts (`operator_names`), lower case.
async fn operator_names(db: &sqlx::PgPool) -> Result<Vec<String>, sqlx::Error> {
    let v: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "WITH {OPS} SELECT name FROM ops"
    )))
    .fetch_all(db)
    .await?;
    Ok(v)
}

fn is_op(ops: &[String], toons: &[String], p: &cache::PlayerRow) -> bool {
    toons.contains(&p.toon)
        || p.name
            .as_deref()
            .is_some_and(|n| ops.contains(&n.to_lowercase()))
}

/// Top `limit` rows per stat over `corpus`, best first (ties: most recent game first). Zero values never count.
pub fn top_records(
    corpus: &Corpus,
    ops: &[String],
    toons: &[String],
    q: &RecordsQuery,
    limit: usize,
) -> Vec<J> {
    let keep: Vec<(&cache::PlayerRow, &cache::Game)> = corpus
        .rows()
        .filter(|(_, g)| q.mode.is_none_or(|m| g.mode == Some(m)))
        .filter(|(_, g)| q.map.as_deref().is_none_or(|m| g.map.as_deref() == Some(m)))
        .filter(|(p, _)| {
            q.hero
                .as_deref()
                .is_none_or(|h| p.hero.as_deref() == Some(h))
        })
        .filter(|(p, _)| !q.mine || is_op(ops, toons, p))
        .collect();
    RECORD_STATS
        .iter()
        .enumerate()
        .map(|(k, (key, label))| {
            let mut rows: Vec<(f64, &cache::PlayerRow, &cache::Game)> = keep
                .iter()
                .filter_map(|(p, g)| Some((p.stats[k].filter(|v| *v > 0.0)?, *p, *g)))
                .collect();
            rows.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.2.played_at.cmp(&a.2.played_at)));
            let rows: Vec<J> = rows
                .into_iter()
                .take(limit)
                .map(|(v, p, g)| {
                    json!({
                        "value": v, "toon": p.toon, "name": p.name, "hero": p.hero, "map": g.map,
                        "mode": g.mode, "played_at": g.played_at, "match_id": g.id,
                        "mine": is_op(ops, toons, p),
                    })
                })
                .collect();
            json!({ "key": key, "label": label, "rows": rows })
        })
        .collect()
}

/// GET /api/records?mode=&map=&hero=&mine= — the best single games per stat (top 5 by default).
pub async fn records(State(s): State<AppState>, Query(q): Query<RecordsQuery>) -> Resp {
    let limit = q.limit.unwrap_or(5).clamp(1, 20) as usize;
    let corpus = cache::corpus(&s.db).await.map_err(db_err)?;
    let ops = operator_names(&s.db).await.map_err(db_err)?;
    let toons = crate::standalone::operator_toons(&s).await;
    Ok(Json(
        json!({ "stats": top_records(&corpus, &ops, &toons, &q, limit) }),
    ))
}

// ── F2: early leads ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AggQuery {
    mode: Option<i32>,
    #[serde(default)]
    mine: bool,
    account: Option<String>,
    from: Option<String>,
    to: Option<String>,
}

/// One game's early leads + the winner + the operator's team, for `tally_leads`.
#[derive(Debug, Clone)]
pub struct LeadGame {
    pub map: String,
    pub winner: Option<u8>,
    pub my_team: Option<u8>,
    pub leads: logic::EarlyLeads,
}

const LEADS: [(&str, &str); 4] = [
    ("first_blood", "First blood"),
    ("first_to_10", "First to level 10"),
    ("first_fort", "First fort"),
    ("first_to_20", "First to level 20"),
];

fn lead_of(l: &logic::EarlyLeads, key: &str) -> Option<u8> {
    match key {
        "first_blood" => l.first_blood,
        "first_to_10" => l.first_to_10,
        "first_fort" => l.first_fort,
        _ => l.first_to_20,
    }
}

fn tally_rows<'a>(games: impl Iterator<Item = &'a LeadGame> + Clone) -> Vec<J> {
    LEADS
        .iter()
        .filter_map(|(key, label)| {
            let (mut n, mut w, mut mn, mut mw) = (0u32, 0u32, 0u32, 0u32);
            for g in games.clone() {
                let (Some(t), Some(win)) = (lead_of(&g.leads, key), g.winner) else {
                    continue;
                };
                n += 1;
                w += u32::from(t == win);
                if g.my_team == Some(t) {
                    mn += 1;
                    mw += u32::from(t == win);
                }
            }
            (n > 0).then(|| {
                json!({ "key": key, "label": label, "games": n, "wins": w, "my_games": mn, "my_wins": mw })
            })
        })
        .collect()
}

/// `{games, leads:[…], maps:[{map, games, leads}]}`: how often getting there first won the game, overall
/// and per map; `my_*` = games where the operator's team got there first. Leads nobody reached are left out.
pub fn tally_leads(games: &[LeadGame]) -> J {
    let mut maps: Vec<&str> = games.iter().map(|g| g.map.as_str()).collect();
    maps.sort_unstable();
    maps.dedup();
    let per_map: Vec<J> = maps
        .into_iter()
        .map(|m| {
            let it = games.iter().filter(move |g| g.map == m);
            json!({ "map": m, "games": it.clone().count(), "leads": tally_rows(it) })
        })
        .collect();
    json!({ "games": games.len(), "leads": tally_rows(games.iter()), "maps": per_map })
}

/// GET /api/maps/early-leads?mode=&mine=&account=&from=&to= — same filters as `/api/maps`.
pub async fn early_leads(State(s): State<AppState>, Query(q): Query<AggQuery>) -> Resp {
    let date = |v: &Option<String>| -> Result<Option<DateTime<Utc>>, Err> {
        v.as_deref()
            .map(|d| d.parse::<DateTime<Utc>>().map_err(|_| bad("date")))
            .transpose()
    };
    let (from, to) = (date(&q.from)?, date(&q.to)?);
    let corpus = cache::corpus(&s.db).await.map_err(db_err)?;
    let ops = operator_names(&s.db).await.map_err(db_err)?;
    let toons = crate::standalone::operator_toons(&s).await;
    let account = q.account.as_deref().map(str::to_lowercase);
    let restrict = q.mine || account.is_some();
    let games: Vec<LeadGame> = corpus
        .games
        .values()
        .filter(|g| g.map.is_some())
        .filter(|g| q.mode.is_none_or(|m| g.mode == Some(m)))
        .filter(|g| from.is_none_or(|f| g.played_at.is_some_and(|p| p >= f)))
        .filter(|g| to.is_none_or(|t| g.played_at.is_some_and(|p| p < t)))
        .filter_map(|g| {
            let me = g.players.iter().find(|p| {
                let n = p.name.as_deref().map(str::to_lowercase);
                match &account {
                    Some(a) => {
                        n.as_deref() == Some(a.as_str())
                            || (toons.contains(&p.toon)
                                && q.account.as_deref().and_then(|a| a.strip_prefix("toon:"))
                                    == Some(p.toon.as_str()))
                    }
                    None => is_op(&ops, &toons, p),
                }
            });
            if restrict && me.is_none() {
                return None;
            }
            Some(LeadGame {
                map: g.map.clone().unwrap_or_default(),
                winner: g.winner,
                my_team: me.and_then(|p| p.team),
                leads: g.leads,
            })
        })
        .collect();
    Ok(Json(tally_leads(&games)))
}

// ── F6: head to head ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CompareQuery {
    a: Option<String>,
    b: Option<String>,
}

/// `{toon, name, games, wins, kills, deaths, assists (averages, 1 decimal), heroes (top 5)}` from the corpus.
pub fn player_card(corpus: &Corpus, toon: &str, name: Option<String>) -> J {
    let rows: Vec<&cache::PlayerRow> = corpus
        .rows()
        .filter(|(p, _)| p.toon == toon)
        .map(|(p, _)| p)
        .collect();
    let avg = |k: usize| {
        let v: Vec<f64> = rows.iter().filter_map(|p| p.stats[k]).collect();
        (!v.is_empty()).then(|| (v.iter().sum::<f64>() / v.len() as f64 * 10.0).round() / 10.0)
    };
    let mut heroes: HashMap<&str, (u32, u32)> = HashMap::new();
    for p in &rows {
        if let Some(h) = p.hero.as_deref() {
            let e = heroes.entry(h).or_default();
            e.0 += 1;
            e.1 += u32::from(p.win);
        }
    }
    let mut heroes: Vec<(&str, u32, u32)> =
        heroes.into_iter().map(|(h, (g, w))| (h, g, w)).collect();
    heroes.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    json!({
        "toon": toon,
        "name": name,
        "games": rows.len(),
        "wins": rows.iter().filter(|p| p.win).count(),
        "kills": avg(0),
        "deaths": avg(3),
        "assists": avg(2),
        "heroes": heroes.iter().take(5).map(|(h, g, w)| json!({"hero": h, "games": g, "wins": w})).collect::<Vec<_>>(),
    })
}

/// GET /api/compare?a=<toon>&b=<toon> — two players side by side, and the games they shared.
pub async fn compare(State(s): State<AppState>, Query(q): Query<CompareQuery>) -> Resp {
    let (Some(a), Some(b)) = (q.a.filter(|x| !x.is_empty()), q.b.filter(|x| !x.is_empty())) else {
        return Err(bad("a_and_b"));
    };
    if a == b {
        return Err(bad("same_player"));
    }
    let corpus = cache::corpus(&s.db).await.map_err(db_err)?;
    let name = |t: &str| {
        let t = t.to_string();
        let db = s.db.clone();
        async move {
            sqlx::query_scalar::<_, Option<String>>(
                "SELECT last_name FROM players WHERE toon_handle = $1",
            )
            .bind(t)
            .fetch_optional(&db)
            .await
            .map(Option::flatten)
        }
    };
    let pa = player_card(&corpus, &a, name(&a).await.map_err(db_err)?);
    let pb = player_card(&corpus, &b, name(&b).await.map_err(db_err)?);
    let shared: J = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(t ORDER BY t.played_at DESC NULLS LAST), '[]'::jsonb) FROM (
           SELECT m.id AS match_id, m.map, m.mode, m.played_at, x.team = y.team AS together,
                  x.hero AS a_hero, y.hero AS b_hero, x.win AS a_win, y.win AS b_win
           FROM match_players x
           JOIN match_players y ON y.match_id = x.match_id AND y.toon_handle = $2
           JOIN matches m ON m.id = x.match_id
           WHERE x.toon_handle = $1) t",
    )
    .bind(&a)
    .bind(&b)
    .fetch_one(&s.db)
    .await
    .map_err(db_err)?;
    let games = shared.as_array().cloned().unwrap_or_default();
    let count = |together: bool, win: bool| {
        games
            .iter()
            .filter(|g| g["together"].as_bool() == Some(together))
            .filter(|g| !win || g["a_win"].as_bool() == Some(true))
            .count()
    };
    Ok(Json(json!({
        "a": pa,
        "b": pb,
        "together": { "games": count(true, false), "wins": count(true, true) },
        "against": { "games": count(false, false), "a_wins": count(false, true) },
        "shared": games.iter().take(50).collect::<Vec<_>>(),
    })))
}

// ── F7: personal insights ────────────────────────────────────────────────────

/// Under this many games the page shows a small-sample warning.
pub const SMALL_SAMPLE: usize = 20;
/// Heroes and maps need this many games to be ranked.
const MIN_GAMES: i64 = 10;

/// One of the player's games, for `personal`.
pub struct PlayerGame {
    pub team: u8,
    pub win: bool,
    pub facts: MatchFacts,
}

/// Nemesis counts, thrown leads, comebacks and isolated deaths of `toon` over `games`.
pub fn personal(toon: &str, games: &[PlayerGame]) -> J {
    let mut killers: HashMap<&str, (u32, std::collections::HashSet<usize>)> = HashMap::new();
    let (mut losses, mut thrown, mut wins, mut comebacks) = (0u32, 0u32, 0u32, 0u32);
    let (mut deaths, mut alone) = (0u32, 0u32);
    for (i, g) in games.iter().enumerate() {
        for d in g.facts.deaths.iter().filter(|d| d.victim == toon) {
            for k in &d.killers {
                let e = killers.entry(k.as_str()).or_default();
                e.0 += 1;
                e.1.insert(i);
            }
        }
        let (up, down) = logic::level_swing(&g.facts, g.team);
        if g.win {
            wins += 1;
            comebacks += u32::from(down >= 2.0);
        } else {
            losses += 1;
            thrown += u32::from(up >= 2.0);
        }
        let fights = logic::team_fights(&g.facts);
        deaths += g.facts.deaths.iter().filter(|d| d.victim == toon).count() as u32;
        alone += logic::isolated_deaths(&g.facts, &fights)
            .iter()
            .filter(|d| d.victim == toon)
            .count() as u32;
    }
    let mut nem: Vec<(&str, u32, usize)> = killers
        .into_iter()
        .map(|(k, (n, g))| (k, n, g.len()))
        .collect();
    nem.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    json!({
        "games": games.len(),
        "small_sample": games.len() < SMALL_SAMPLE,
        "nemesis": nem.iter().take(3).map(|(t, k, g)| json!({"toon": t, "kills": k, "games": g})).collect::<Vec<_>>(),
        "thrown": { "losses": losses, "count": thrown },
        "comebacks": { "wins": wins, "count": comebacks },
        "deaths": { "total": deaths, "isolated": alone },
    })
}

/// Maps or heroes with at least `MIN_GAMES` games: `{name, games, wins, pct}`, best win rate first.
pub fn ranked(rows: impl Iterator<Item = (String, bool)>) -> Vec<J> {
    let mut by: HashMap<String, (u32, u32)> = HashMap::new();
    for (k, win) in rows {
        let e = by.entry(k).or_default();
        e.0 += 1;
        e.1 += u32::from(win);
    }
    let mut v: Vec<(String, u32, u32, f64)> = by
        .into_iter()
        .filter(|(_, (g, _))| i64::from(*g) >= MIN_GAMES)
        .map(|(k, (g, w))| (k, g, w, (100.0 * f64::from(w) / f64::from(g)).round()))
        .collect();
    v.sort_by(|a, b| b.3.total_cmp(&a.3).then(b.1.cmp(&a.1)).then(a.0.cmp(&b.0)));
    v.into_iter()
        .map(|(name, games, wins, pct)| json!({ "name": name, "games": games, "wins": wins, "pct": pct }))
        .collect()
}

/// GET /api/players/{toon}/insights — nemesis, thrown leads, comebacks, isolated deaths, best/worst maps
/// and heroes (≥ 10 games).
pub async fn player_insights(State(s): State<AppState>, Path(toon): Path<String>) -> Resp {
    let corpus = cache::corpus(&s.db).await.map_err(db_err)?;
    let mine: Vec<(&cache::PlayerRow, &cache::Game)> =
        corpus.rows().filter(|(p, _)| p.toon == toon).collect();
    let games: Vec<PlayerGame> = mine
        .iter()
        .filter_map(|(p, g)| {
            Some(PlayerGame {
                team: p.team?,
                win: p.win,
                facts: corpus.facts(g),
            })
        })
        .collect();
    let mut out = personal(&toon, &games);
    // latest name of each nemesis
    if let Some(list) = out["nemesis"].as_array_mut() {
        for n in list.iter_mut() {
            let t = n["toon"].as_str().unwrap_or_default();
            let name = corpus
                .rows()
                .filter(|(p, _)| p.toon == t && p.name.is_some())
                .max_by_key(|(_, g)| g.played_at)
                .and_then(|(p, _)| p.name.clone());
            n["name"] = json!(name);
        }
    }
    out["maps"] = json!(ranked(
        mine.iter()
            .filter_map(|(p, g)| Some((g.map.clone()?, p.win)))
    ));
    out["heroes"] = json!(ranked(
        mine.iter()
            .filter_map(|(p, _)| Some((p.hero.clone()?, p.win)))
    ));
    Ok(Json(out))
}

// ── Session review ───────────────────────────────────────────────────────────

use super::session::{self, Window};

#[derive(Deserialize)]
pub struct SessionsQuery {
    account: Option<String>,
    limit: Option<i64>,
    prefer_open: Option<bool>,
    automatic_only: Option<bool>,
}

async fn manual_windows(s: &AppState) -> Result<Vec<Window>, Err> {
    type Row = (i64, DateTime<Utc>, Option<DateTime<Utc>>, Option<String>);
    let rows = sqlx::query_as::<_, Row>(
        "SELECT -id, started_at, ended_at, name FROM manual_sessions ORDER BY started_at, id",
    )
    .fetch_all(&s.db)
    .await
    .map_err(db_err)?;
    Ok(rows
        .into_iter()
        .map(|(id, start, end, name)| Window {
            id,
            start,
            end,
            name,
        })
        .collect())
}

async fn session_assignments(
    s: &AppState,
    q: &SessionsQuery,
) -> Result<(Vec<Window>, HashMap<i64, Option<i64>>), Err> {
    if q.automatic_only == Some(true) {
        return Ok((Vec::new(), HashMap::new()));
    }
    // Read overrides first: their targets are guaranteed to exist in the later window read.
    let rows = sqlx::query_as::<_, (i64, Option<i64>)>(
        "SELECT match_id, -manual_session_id FROM match_session_overrides",
    )
    .fetch_all(&s.db)
    .await
    .map_err(db_err)?;
    Ok((manual_windows(s).await?, rows.into_iter().collect()))
}

/// Manual targets, including empty historical windows, for the operator's management UI.
pub async fn session_windows(State(s): State<AppState>, Query(q): Query<SessionsQuery>) -> Resp {
    require_default_view(&q)?;
    let sessions: Vec<J> = manual_windows(&s)
        .await?
        .into_iter()
        .rev()
        .map(|w| {
            let mut item = json!(w.meta());
            item["id"] = json!(w.id);
            item
        })
        .collect();
    Ok(Json(json!({ "sessions": sessions })))
}

fn require_default_view(q: &SessionsQuery) -> Result<(), Err> {
    if q.account.is_some() {
        Err(bad("account_read_only"))
    } else {
        Ok(())
    }
}

fn session_error(status: StatusCode, error: &str) -> Err {
    (status, Json(json!({"error": error})))
}

// Serialize window transitions, including a start racing an end. Take timestamps after acquiring
// the lock so a later window cannot begin before the previous one ends.
async fn lock_session_windows(s: &AppState) -> Result<sqlx::Transaction<'_, sqlx::Postgres>, Err> {
    let mut tx = s.db.begin().await.map_err(db_err)?;
    sqlx::query("LOCK TABLE manual_sessions IN SHARE ROW EXCLUSIVE MODE")
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    Ok(tx)
}

pub async fn start_session(
    State(s): State<AppState>,
    Query(q): Query<SessionsQuery>,
) -> Result<(StatusCode, Json<J>), Err> {
    require_default_view(&q)?;
    let mut tx = lock_session_windows(&s).await?;
    let id: Option<i64> = sqlx::query_scalar(
        "INSERT INTO manual_sessions (started_at)
         SELECT greatest(clock_timestamp(), max(ended_at)) FROM manual_sessions
         ON CONFLICT ((true)) WHERE ended_at IS NULL DO NOTHING RETURNING -id",
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    let id = id.ok_or_else(|| session_error(StatusCode::CONFLICT, "session_already_open"))?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

pub async fn end_session(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<SessionsQuery>,
) -> Resp {
    require_default_view(&q)?;
    let stored = stored_session_id(id)?;
    let mut tx = lock_session_windows(&s).await?;
    let found: Option<i64> = sqlx::query_scalar(
        "UPDATE manual_sessions SET ended_at = coalesce(ended_at, greatest(started_at, clock_timestamp()))
         WHERE id = $1 RETURNING -id",
    ).bind(stored).fetch_optional(&mut *tx).await.map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    found
        .map(|id| Json(json!({ "id": id })))
        .ok_or_else(|| session_error(StatusCode::NOT_FOUND, "not_found"))
}

fn stored_session_id(id: i64) -> Result<i64, Err> {
    id.checked_neg()
        .filter(|&id| id > 0)
        .ok_or_else(|| bad("manual_session_id"))
}

#[derive(Deserialize)]
pub struct SessionName {
    name: Option<String>,
}

pub async fn rename_session(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<SessionsQuery>,
    Json(body): Json<SessionName>,
) -> Resp {
    require_default_view(&q)?;
    let stored = stored_session_id(id)?;
    let name = body
        .name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty());
    if name.is_some_and(|n| n.chars().count() > 80 || n.contains('\0')) {
        return Err(bad("session_name"));
    }
    let found: Option<i64> =
        sqlx::query_scalar("UPDATE manual_sessions SET name = $2 WHERE id = $1 RETURNING -id")
            .bind(stored)
            .bind(name)
            .fetch_optional(&s.db)
            .await
            .map_err(db_err)?;
    found
        .map(|id| Json(json!({ "id": id })))
        .ok_or_else(|| session_error(StatusCode::NOT_FOUND, "not_found"))
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Assignment {
    Manual { session_id: i64 },
    Automatic,
    Window,
}

pub async fn assign_session(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<SessionsQuery>,
    Json(body): Json<Assignment>,
) -> Resp {
    require_default_view(&q)?;
    let toons = crate::standalone::operator_toons(&s).await;
    let target = match body {
        Assignment::Manual { session_id } => {
            let stored = stored_session_id(session_id)?;
            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM manual_sessions WHERE id = $1)")
                    .bind(stored)
                    .fetch_one(&s.db)
                    .await
                    .map_err(db_err)?;
            if !exists {
                return Err(session_error(
                    StatusCode::NOT_FOUND,
                    "manual_session_not_found",
                ));
            }
            Some(stored)
        }
        _ => None,
    };
    // Test eligibility on the same statement snapshot as the write, not on the cached corpus.
    let action = match body {
        Assignment::Window => "DELETE FROM match_session_overrides WHERE match_id IN (SELECT id FROM eligible) RETURNING match_id",
        _ => "INSERT INTO match_session_overrides (match_id, manual_session_id)
              SELECT id, $3 FROM eligible ON CONFLICT (match_id) DO UPDATE SET manual_session_id = excluded.manual_session_id RETURNING match_id",
    };
    // Match project_once's lock order: lock the match before deleting its override.
    // If projection replaced the row while we waited, FOR UPDATE yields no eligible
    // match. A zero-row DELETE is otherwise fine (an idempotent clear).
    let lock = if matches!(body, Assignment::Window) {
        "FOR UPDATE OF m"
    } else {
        ""
    };
    let sql = format!(
        "WITH {OPS}, eligible AS (
        SELECT m.id FROM matches m WHERE m.id = $1 AND m.played_at IS NOT NULL AND EXISTS (
            SELECT 1 FROM match_players p WHERE p.match_id = m.id
              AND (p.toon_handle = ANY($2) OR lower(p.name) IN (SELECT name FROM ops))
        ) {lock}
    ), applied AS ({action}) SELECT EXISTS (SELECT 1 FROM eligible)"
    );
    let query = sqlx::query_scalar::<_, bool>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(&toons);
    let query = if matches!(body, Assignment::Window) {
        query
    } else {
        query.bind(target)
    };
    if !query.fetch_one(&s.db).await.map_err(db_err)? {
        return Err(session_error(
            StatusCode::NOT_FOUND,
            "eligible_match_not_found",
        ));
    }
    Ok(Json(json!({ "match_id": id })))
}

/// The operator's dated games (or one account's), oldest first; 400 `account` on an unknown account.
async fn session_games(
    s: &AppState,
    account: Option<&str>,
) -> Result<Vec<super::session::MyGame>, Err> {
    let ops = operator_names(&s.db).await.map_err(db_err)?;
    let toons = crate::standalone::operator_toons(s).await;
    let who = super::session::Perspective {
        ops: &ops,
        toons: &toons,
        account,
    };
    if !who.known() {
        return Err(bad("account"));
    }
    let corpus = cache::corpus(&s.db).await.map_err(db_err)?;
    Ok(super::session::my_games(&corpus, &who))
}

/// GET /api/sessions?account=&limit= — the operator's play sessions, newest first (`limit` 20, max 100).
pub async fn sessions(State(s): State<AppState>, Query(q): Query<SessionsQuery>) -> Resp {
    let limit = q.limit.unwrap_or(20).clamp(1, 100) as usize;
    let games = session_games(&s, q.account.as_deref()).await?;
    let (windows, overrides) = session_assignments(&s, &q).await?;
    let groups = session::groups(&games, &windows, &overrides, q.account.is_some());
    Ok(Json(
        json!({ "sessions": session::list(&groups, limit, q.prefer_open == Some(true)) }),
    ))
}

/// GET /api/sessions/{id}?account= — one session against the 90 days before it.
pub async fn session_review(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<SessionsQuery>,
) -> Resp {
    let games = session_games(&s, q.account.as_deref()).await?;
    let (windows, overrides) = session_assignments(&s, &q).await?;
    let groups = session::groups(&games, &windows, &overrides, q.account.is_some());
    match session::review(&games, &groups, id) {
        Some(r) => Ok(Json(json!(r))),
        None => Err((StatusCode::NOT_FOUND, Json(json!({ "error": "not_found" })))),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use logic::EarlyLeads;

    #[tokio::test]
    async fn manual_session_starts_empty_and_rejects_a_second_start() {
        use crate::api_tests::{call, test_state};
        let Some(s) = test_state(None).await else {
            return;
        };
        let (a, b) = tokio::join!(
            call(&s, "POST", "/api/sessions", None),
            call(&s, "POST", "/api/sessions", None),
        );
        let (created, conflict) = if a.0 == StatusCode::CREATED {
            (a, b)
        } else {
            (b, a)
        };
        assert_eq!(created.0, StatusCode::CREATED);
        assert_eq!(conflict.0, StatusCode::CONFLICT);
        let id = created.1["id"].as_i64().unwrap();
        assert!(id < 0);
        let detail = format!("/api/sessions/{id}");
        let (status, review) = call(&s, "GET", &detail, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(review["open"], true);
        assert_eq!(review["games"], json!([]));
        assert_eq!(review["baseline"]["averages"], json!({}));
        assert_eq!(
            call(&s, "POST", &format!("{detail}/end"), None).await.0,
            StatusCode::OK
        );
    }

    fn lg(map: &str, winner: u8, my: Option<u8>, fb: Option<u8>, f10: Option<u8>) -> LeadGame {
        LeadGame {
            map: map.into(),
            winner: Some(winner),
            my_team: my,
            leads: EarlyLeads {
                first_blood: fb,
                first_to_10: f10,
                ..Default::default()
            },
        }
    }

    #[test]
    fn tallies_early_leads_overall_per_map_and_for_me() {
        let games = [
            lg("A", 0, Some(0), Some(0), Some(0)),
            lg("A", 1, Some(0), Some(0), Some(1)),
            lg("B", 1, None, Some(1), None),
        ];
        let t = tally_leads(&games);
        assert_eq!(t["games"], 3);
        let fb = &t["leads"][0];
        assert_eq!(fb["key"], "first_blood");
        assert_eq!(
            (fb["games"].as_u64(), fb["wins"].as_u64()),
            (Some(3), Some(2))
        );
        assert_eq!(
            (fb["my_games"].as_u64(), fb["my_wins"].as_u64()),
            (Some(2), Some(1))
        );
        // nobody reached a fort or level 20: those rows are left out
        assert_eq!(t["leads"].as_array().unwrap().len(), 2);
        assert_eq!(t["maps"][0]["map"], "A");
        assert_eq!(t["maps"][1]["leads"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn personal_counts_nemesis_throws_and_lone_deaths() {
        let teams: HashMap<String, u8> = [("me", 0u8), ("al", 0), ("x", 1), ("y", 1)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        let td = |t: f64, x: f64, victim: &str, killers: &[&str]| {
            json!({"time": t, "x": x * 4096.0, "y": 0.0, "victim": {"player": victim},
                   "killers": killers.iter().map(|k| json!({"player": k})).collect::<Vec<_>>()})
        };
        let g1 = json!({
            "takedowns": [td(10.0, 0.0, "me", &["x", "y"]), td(12.0, 1.0, "al", &["x"]), td(200.0, 0.0, "me", &["x"])],
            "levelAdvTimeline": [{"start": 0, "end": 100, "levelDiff": 3.0}]
        });
        let g2 = json!({
            "takedowns": [td(50.0, 0.0, "me", &["y"])],
            "levelAdvTimeline": [{"start": 0, "end": 100, "levelDiff": -2.0}]
        });
        let games = [
            PlayerGame {
                team: 0,
                win: false,
                facts: MatchFacts::from_match(&g1, &teams),
            },
            PlayerGame {
                team: 0,
                win: true,
                facts: MatchFacts::from_match(&g2, &teams),
            },
        ];
        let p = personal("me", &games);
        assert_eq!(p["nemesis"][0]["toon"], "x");
        assert_eq!(p["nemesis"][0]["kills"], 2);
        assert_eq!(p["nemesis"][0]["games"], 1);
        assert_eq!(p["nemesis"][1]["toon"], "y");
        assert_eq!(p["nemesis"][1]["games"], 2);
        assert_eq!(p["thrown"]["count"], 1);
        assert_eq!(p["comebacks"]["count"], 1);
        // death at 10 s is in a fight (with al at 12 s); 200 s and the other game's one are alone
        assert_eq!(p["deaths"]["total"], 3);
        assert_eq!(p["deaths"]["isolated"], 2);
        assert_eq!(p["small_sample"], true);
    }

    /// The five endpoints on a real committed replay (DB; skipped without `DATABASE_URL`).
    #[test]
    fn ranks_maps_and_heroes_with_enough_games() {
        let rows = (0..12)
            .map(|i| ("A".to_string(), i < 9))
            .chain((0..10).map(|i| ("B".to_string(), i < 3)))
            .chain((0..5).map(|_| ("C".to_string(), true)));
        let r = ranked(rows);
        assert_eq!(r.len(), 2, "C has under 10 games");
        assert_eq!(
            (r[0]["name"].as_str(), r[0]["pct"].as_f64()),
            (Some("A"), Some(75.0))
        );
        assert_eq!(r[1]["name"], "B");
    }

    /// The corpus catches up with a new game even when other games disappear.
    #[tokio::test]
    async fn corpus_refreshes_incrementally() {
        let Some(state) = crate::api_tests::test_state(None).await else {
            return;
        };
        let tag = uuid::Uuid::new_v4().simple().to_string();
        let retired: Vec<i64> = sqlx::query_scalar(
            "INSERT INTO matches (fingerprint, parser_version, data, map, mode)
             VALUES ($1, 0, '{}', 'Test Map', 50091), ($2, 0, '{}', 'Test Map', 50091) RETURNING id",
        )
        .bind(format!("insights-{tag}-retired-a"))
        .bind(format!("insights-{tag}-retired-b"))
        .fetch_all(&state.db)
        .await
        .unwrap();
        let before = cache::corpus(&state.db).await.unwrap();

        // Simulate another test cleaning up its fixtures between corpus reads.
        let mut change = state.db.begin().await.unwrap();
        sqlx::query("DELETE FROM matches WHERE id = ANY($1)")
            .bind(&retired)
            .execute(&mut *change)
            .await
            .unwrap();
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO matches (fingerprint, parser_version, data, map, mode) VALUES ($1, 0, '{}', 'Test Map', 50091) RETURNING id",
        )
        .bind(format!("insights-{tag}"))
        .fetch_one(&mut *change)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO match_players (match_id, toon_handle, name, hero, team, win, kills, data)
             VALUES ($1, $2, 'x', 'Valla', 0, true, 7, '{\"gameStats\": {\"Assists\": 3}}')",
        )
        .bind(id)
        .bind(format!("toon-{tag}"))
        .execute(&mut *change)
        .await
        .unwrap();
        change.commit().await.unwrap();
        let after = cache::corpus(&state.db).await.unwrap();
        let again = cache::corpus(&state.db).await.unwrap();
        sqlx::query("DELETE FROM matches WHERE id = $1")
            .bind(id)
            .execute(&state.db)
            .await
            .unwrap();
        std::fs::remove_dir_all(state.cfg.archive_dir.parent().unwrap()).unwrap();

        assert!(!before.games.contains_key(&id));
        for retired_id in retired {
            assert!(before.games.contains_key(&retired_id));
            assert!(!after.games.contains_key(&retired_id));
        }
        // Other tests also write to this database; check only the rows we own.
        for snapshot in [&after, &again] {
            let rows = &snapshot.games[&id].players;
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].toon, format!("toon-{tag}"));
            assert_eq!(rows[0].stats[0], Some(7.0));
            assert_eq!(rows[0].stats[2], Some(3.0));
        }
    }

    #[tokio::test]
    async fn endpoints_answer_on_a_real_game() {
        let Some(state) = crate::api_tests::test_state(None).await else {
            return;
        };
        let replay = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../storm-replay/tests/data/2026-06-09 20.35.02 Industrial District.StormReplay");
        let out = storm_stats::process_replay(&replay, "x");
        let fp = crate::upload::game_fingerprint(&out).unwrap();
        let id = crate::project::project_match(&state.db, &fp, crate::PARSER_VERSION, &out)
            .await
            .unwrap();
        let call = |path: String| {
            let state = state.clone();
            async move { crate::api_tests::call(&state, "GET", &path, None).await }
        };

        let (st, m) = call(format!("/api/matches/{id}/insights")).await;
        assert_eq!(st, StatusCode::OK, "{m}");
        assert!(m["fights"].is_array() && m["camps"].is_array());
        assert!(m["window"].is_null());
        assert_eq!(m["teamfight_totals"].as_array().unwrap().len(), 10);
        let (st, w) = call(format!("/api/matches/{id}/insights?from=0&to=10000")).await;
        assert_eq!(st, StatusCode::OK);
        // the whole game as a window: every death is in it
        let deaths: u64 = w["window"]["players"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["deaths"].as_u64().unwrap())
            .sum();
        let takedowns = out
            .match_
            .as_ref()
            .and_then(|m| m.get("takedowns"))
            .and_then(J::as_array)
            .map_or(0, Vec::len) as u64;
        assert_eq!(deaths, takedowns);
        assert_eq!(
            call(format!("/api/matches/{id}/insights?from=1")).await.0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            call("/api/matches/0/insights".into()).await.0,
            StatusCode::NOT_FOUND
        );

        let (st, r) = call("/api/records?map=Industrial%20District".into()).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(r["stats"].as_array().unwrap().len(), RECORD_STATS.len());
        let top = &r["stats"][0]["rows"];
        assert!(top.as_array().unwrap().len() <= 5);

        let (st, e) = call("/api/maps/early-leads".into()).await;
        assert_eq!(st, StatusCode::OK);
        assert!(e["games"].as_u64().unwrap() >= 1);

        let toons: Vec<String> = sqlx::query_scalar(
            "SELECT toon_handle FROM match_players WHERE match_id = $1 ORDER BY team, id",
        )
        .bind(id)
        .fetch_all(&state.db)
        .await
        .unwrap();
        let (a, b) = (&toons[0], &toons[9]);
        let (st, c) = call(format!("/api/compare?a={a}&b={b}")).await;
        assert_eq!(st, StatusCode::OK, "{c}");
        assert!(c["against"]["games"].as_u64().unwrap() >= 1);
        assert_eq!(
            call(format!("/api/compare?a={a}&b={a}")).await.0,
            StatusCode::BAD_REQUEST
        );

        let (st, p) = call(format!("/api/players/{a}/insights")).await;
        assert_eq!(st, StatusCode::OK, "{p}");
        assert!(p["games"].as_u64().unwrap() >= 1);
        assert!(p["maps"].is_array() && p["heroes"].is_array());
    }

    /// One game of `toon` (team 0) against a filler row, for the session endpoints.
    #[allow(clippy::too_many_arguments)]
    async fn session_game(
        db: &mut sqlx::PgConnection,
        toon: &str,
        name: &str,
        at: DateTime<Utc>,
        hero: &str,
        win: bool,
        kills: i32,
        deaths: i32,
    ) -> i64 {
        let tag = uuid::Uuid::new_v4().simple().to_string();
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO matches (fingerprint, parser_version, data, played_at, map, mode, length)
             VALUES ($1, 0, '{}', $2, 'Sky Temple', 50091, 1200) RETURNING id",
        )
        .bind(format!("session-{tag}"))
        .bind(at)
        .fetch_one(&mut *db)
        .await
        .unwrap();
        for (t, n, team, w) in [
            (toon.to_string(), name.to_string(), 0, win),
            (format!("other-{tag}"), "Other".to_string(), 1, !win),
        ] {
            sqlx::query(
                "INSERT INTO match_players (match_id, toon_handle, name, hero, team, win, kills, deaths,
                                            takedowns, hero_damage, healing, experience, data)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $7 + 4, 1000 * $7, 0, 9000,
                         '{\"gameStats\": {\"SiegeDamage\": 20000, \"awards\": [\"EndOfMatchAwardMVPBoolean\"]}}')",
            )
            .bind(id)
            .bind(t)
            .bind(n)
            .bind(hero)
            .bind(team)
            .bind(w)
            .bind(kills)
            .bind(deaths)
            .execute(&mut *db)
            .await
            .unwrap();
        }
        id
    }

    #[tokio::test]
    async fn manual_assignment_reads_late_games_and_enforces_perspective() {
        use crate::api_tests::{app, call, test_state, test_state_with_db};
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;
        let Some(s) = test_state(None).await else {
            return;
        };
        let hosted = test_state_with_db(
            s.cfg.database_url.clone(),
            s.db.clone(),
            Some("fixture-admin"),
        );
        for (method, path) in [
            ("POST", "/api/sessions"),
            ("POST", "/api/sessions/-1/end"),
            ("PATCH", "/api/sessions/-1"),
            ("PUT", "/api/sessions/assignments/1"),
        ] {
            assert_eq!(
                call(&hosted, method, path, None).await.0,
                StatusCode::UNAUTHORIZED
            );
        }
        let tag = uuid::Uuid::new_v4().simple().to_string();
        let name = format!("Manual{tag}");
        let other = format!("Other{tag}");
        // Far in the past: unfiltered reads see every operator game, including other tests' fixtures.
        let start = "2001-03-04T12:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let end = start + chrono::Duration::hours(1);
        let mut tx = s.db.begin().await.unwrap();
        let first: i64 = sqlx::query_scalar(
            "INSERT INTO manual_sessions (started_at, ended_at) VALUES ($1, $2) RETURNING -id",
        )
        .bind(start)
        .bind(end)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        let second: i64 = sqlx::query_scalar(
            "INSERT INTO manual_sessions (started_at, ended_at) VALUES ($1, $2) RETURNING -id",
        )
        .bind(end + chrono::Duration::days(1))
        .bind(end + chrono::Duration::days(2))
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        // PostgreSQL stores timestamps to the microsecond; seed exactly at its stored edges.
        let (start, end): (DateTime<Utc>, DateTime<Utc>) =
            sqlx::query_as("SELECT started_at, ended_at FROM manual_sessions WHERE id = $1")
                .bind(-first)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        sqlx::query("INSERT INTO app_settings (key, value) VALUES ('operator_names', jsonb_build_array($1::text, $2::text))
            ON CONFLICT (key) DO UPDATE SET value = app_settings.value || EXCLUDED.value")
            .bind(&name).bind(&other).execute(&mut *tx).await.unwrap();
        let before = session_game(
            &mut tx,
            &tag,
            &name,
            start - chrono::Duration::minutes(1),
            "Valla",
            false,
            13,
            2,
        )
        .await;
        let baseline = session_game(
            &mut tx,
            &tag,
            &name,
            start - chrono::Duration::days(1),
            "Valla",
            true,
            3,
            2,
        )
        .await;
        let at_start = session_game(&mut tx, &tag, &name, start, "Valla", true, 9, 2).await;
        let at_end = session_game(&mut tx, &tag, &name, end, "Valla", false, 2, 2).await;
        let other_id = session_game(&mut tx, &other, &other, start, "Jaina", true, 1, 2).await;
        let stranger = session_game(&mut tx, "stranger", &tag, start, "Jaina", true, 1, 2).await;
        tx.commit().await.unwrap();
        let detail = format!("/api/sessions/{first}");
        let assignment = |id| format!("/api/sessions/assignments/{id}");
        let ids = |r: &J| {
            r["games"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| m["match_id"].as_i64().unwrap())
                .collect::<Vec<_>>()
        };
        let (status, review) = call(&s, "GET", &format!("{detail}?account={name}"), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(ids(&review), [at_start]); // late ingestion; start included, end excluded, other account absent
        assert_eq!(
            call(
                &s,
                "GET",
                &format!("/api/sessions/{second}?account={name}"),
                None
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            call(&s, "PATCH", &detail, Some(json!({"name": "é".repeat(81)})))
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
        let req = Request::patch(&detail)
            .header("authorization", "Bearer fixture-admin")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"name":"  Evening  "}"#))
            .unwrap();
        assert_eq!(
            app(&hosted).oneshot(req).await.unwrap().status(),
            StatusCode::OK
        );
        assert_eq!(
            call(&hosted, "GET", &detail, None).await.1["name"],
            "Evening"
        );
        let assign = |id| Some(json!({"kind":"manual", "session_id": id}));
        assert_eq!(
            call(&s, "PUT", &assignment(stranger), assign(first))
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            call(&s, "PUT", &assignment(at_start), assign(i64::MIN + 1))
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            call(
                &s,
                "PUT",
                &format!("{}?account={name}", assignment(at_start)),
                assign(second)
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            call(&s, "PUT", &assignment(before), assign(first)).await.0,
            StatusCode::OK
        );
        let review = call(&s, "GET", &format!("{detail}?account={name}"), None)
            .await
            .1;
        assert_eq!(ids(&review), [before, at_start]);
        assert_eq!(review["baseline"]["games"], 1);
        assert_eq!(review["baseline"]["averages"]["kills"], 3.0);
        assert_eq!(
            call(&s, "PUT", &assignment(at_start), assign(second))
                .await
                .0,
            StatusCode::OK
        );
        assert_eq!(
            ids(&call(&s, "GET", &format!("/api/sessions/{second}"), None)
                .await
                .1),
            [at_start]
        );
        assert_eq!(
            ids(&call(&s, "GET", &format!("{detail}?account={name}"), None)
                .await
                .1),
            [before]
        );
        assert_eq!(
            call(
                &s,
                "PUT",
                &assignment(at_start),
                Some(json!({"kind":"automatic"}))
            )
            .await
            .0,
            StatusCode::OK
        );
        assert_eq!(
            ids(&call(
                &s,
                "GET",
                &format!("/api/sessions/{at_start}?account={name}"),
                None
            )
            .await
            .1),
            [at_start, at_end]
        );
        assert_eq!(
            call(
                &s,
                "PUT",
                &assignment(at_start),
                Some(json!({"kind":"window"}))
            )
            .await
            .0,
            StatusCode::OK
        );
        assert_eq!(
            ids(&call(&s, "GET", &format!("{detail}?account={name}"), None)
                .await
                .1),
            [before, at_start]
        );

        sqlx::query("DELETE FROM matches WHERE id = ANY($1)")
            .bind(&[before, baseline, at_start, at_end, other_id, stranger][..])
            .execute(&s.db)
            .await
            .unwrap();
        sqlx::query("DELETE FROM manual_sessions WHERE id = ANY($1)")
            .bind(&[-first, -second][..])
            .execute(&s.db)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn session_endpoints_list_review_and_navigate() {
        let Some(state) = crate::api_tests::test_state(None).await else {
            return;
        };
        let tag = uuid::Uuid::new_v4().simple().to_string();
        let (toon, name) = (format!("sess-{tag}"), format!("Sess{tag}"));
        let mut fixture = state.db.begin().await.unwrap();
        // add the account to `operator_names` without dropping the other tests' names
        sqlx::query(
            "INSERT INTO app_settings (key, value) VALUES ('operator_names', jsonb_build_array($1::text))
             ON CONFLICT (key) DO UPDATE SET value = app_settings.value || EXCLUDED.value",
        )
        .bind(&name)
        .execute(&mut *fixture)
        .await
        .unwrap();
        let t0 = chrono::TimeZone::with_ymd_and_hms(&Utc, 2021, 3, 1, 20, 0, 0).unwrap();
        let h = chrono::Duration::hours;
        let mut ids = Vec::new();
        // baseline: 5 Valla games in the 90 days before, 6 kills each
        for d in 1..=5 {
            ids.push(
                session_game(
                    &mut fixture,
                    &toon,
                    &name,
                    t0 - chrono::Duration::days(d),
                    "Valla",
                    d % 2 == 0,
                    6,
                    3,
                )
                .await,
            );
        }
        // session 1: two games 1 h apart; session 2: 4 h later, three games
        let s1 = session_game(&mut fixture, &toon, &name, t0, "Valla", true, 9, 1).await;
        let s1b = session_game(&mut fixture, &toon, &name, t0 + h(1), "Jaina", false, 2, 6).await;
        let s2 = session_game(&mut fixture, &toon, &name, t0 + h(5), "Valla", true, 7, 2).await;
        ids.push(session_game(&mut fixture, &toon, &name, t0 + h(6), "Valla", true, 4, 5).await);
        ids.push(session_game(&mut fixture, &toon, &name, t0 + h(7), "Valla", false, 5, 4).await);
        ids.extend([s1, s1b, s2]);
        fixture.commit().await.unwrap();
        let call = |path: String| {
            let state = state.clone();
            async move { crate::api_tests::call(&state, "GET", &path, None).await }
        };
        let acc = format!("account={name}");

        let (st, l) = call(format!("/api/sessions?{acc}")).await;
        let (one_st, one) = call(format!("/api/sessions?{acc}&limit=1")).await;
        let (review_st, r) = call(format!("/api/sessions/{s1}?{acc}")).await;
        let (review2_st, r2) = call(format!("/api/sessions/{s2}?{acc}")).await;
        let (not_found, _) = call(format!("/api/sessions/{s1b}?{acc}")).await;
        let (unknown_account, e) = call(format!("/api/sessions?account=nobody-{tag}")).await;
        let (unverified_toon, _) = call(format!("/api/sessions/{s1}?account=toon%3A{toon}")).await;

        // Clean up before assertions so a regression cannot leave matches or operator names behind.
        let mut cleanup = state.db.begin().await.unwrap();
        sqlx::query("DELETE FROM matches WHERE id = ANY($1)")
            .bind(&ids)
            .execute(&mut *cleanup)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE app_settings SET value = value - $1::text WHERE key = 'operator_names'",
        )
        .bind(&name)
        .execute(&mut *cleanup)
        .await
        .unwrap();
        cleanup.commit().await.unwrap();
        std::fs::remove_dir_all(state.cfg.archive_dir.parent().unwrap()).unwrap();

        assert_eq!(st, StatusCode::OK, "{l}");
        let list = l["sessions"].as_array().unwrap();
        // 5 one-game baseline sessions (a day apart) + the two sessions
        assert_eq!(list.len(), 7);
        assert_eq!(list[0]["id"], s2, "newest first");
        assert_eq!(
            (list[0]["games"].as_u64(), list[0]["wins"].as_u64()),
            (Some(3), Some(2))
        );
        assert_eq!(list[1]["id"], s1);
        assert_eq!(one_st, StatusCode::OK);
        assert_eq!(one["sessions"].as_array().unwrap().len(), 1);

        assert_eq!(review_st, StatusCode::OK, "{r}");
        assert_eq!(r["record"], json!({"wins": 1, "losses": 1}));
        assert_eq!(r["games"][0]["match_id"], s1);
        assert_eq!(r["games"][0]["baseline"], "hero");
        assert_eq!(r["games"][0]["deltas"]["kills"], 3.0);
        assert_eq!(r["games"][0]["assists"], 4.0);
        assert_eq!(r["games"][0]["award"], "EndOfMatchAwardMVPBoolean");
        assert_eq!(r["games"][1]["baseline"], "overall");
        assert_eq!(
            (r["best"].as_i64(), r["worst"].as_i64()),
            (Some(s1), Some(s1b))
        );
        assert_eq!(r["baseline"]["games"], 5);
        assert_eq!(r["baseline"]["small_sample"], true);
        assert_eq!(r["baseline"]["modes"], json!([50091]));
        assert_eq!(r["span_s"], 4800.0);
        assert_eq!(r["played_s"], 2400.0);
        let valla = r["heroes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["name"] == "Valla")
            .unwrap();
        assert_eq!(
            (
                valla["base_games"].as_u64(),
                valla["small_sample"].as_bool()
            ),
            (Some(5), Some(false))
        );
        assert_eq!(r["next"], s2);
        assert!(r["prev"].is_i64());

        assert_eq!(review2_st, StatusCode::OK);
        assert_eq!(r2["prev"], s1);
        assert!(r2["next"].is_null());
        assert_eq!(
            r2["streaks"],
            json!({"best_win": 2, "worst_loss": 1, "current": -1})
        );

        assert_eq!(not_found, StatusCode::NOT_FOUND);
        assert_eq!(
            (unknown_account, e["error"].as_str()),
            (StatusCode::BAD_REQUEST, Some("account"))
        );
        assert_eq!(
            unverified_toon,
            StatusCode::BAD_REQUEST,
            "a toon is only an account when a folder verifies it"
        );
    }
}
