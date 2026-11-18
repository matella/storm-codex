//! In-memory corpus for the insights that scan every game (records, early leads, personal insights).
//!
//! Reading a stat out of `match_players.data` detoasts the whole JSON (~8 kB a row): one pass over the box's
//! 35k rows takes ~1 s, so computing records per request took 12 s. The corpus extracts what those endpoints
//! need once, keeps it in memory, and catches up with new games incrementally: each request first reads a
//! cheap version of `matches` (count, max id, sum of parser versions — no JSON). New ids only → load just those
//! games; anything else (re-process, deletion) → full reload. Nothing is written anywhere.

use super::logic::{self, Death, EarlyLeads, MatchFacts};
use chrono::{DateTime, Utc};
use serde_json::{json, Value as J};
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use tokio::sync::Mutex;

/// Stats kept per player row, in `RECORD_STATS` order (see `api.rs`).
pub const N_STATS: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Version {
    pub count: i64,
    pub max_id: i64,
    pub parser_sum: i64,
}

#[derive(Debug, Clone)]
pub struct PlayerRow {
    pub match_id: i64,
    pub toon: String,
    pub name: Option<String>,
    pub hero: Option<String>,
    pub team: Option<u8>,
    pub win: bool,
    pub stats: [Option<f64>; N_STATS],
}

/// A death, compact: toons are indices into `Corpus::toons` (full `MatchFacts` cost ~10× the memory).
#[derive(Debug, Clone)]
pub struct SlimDeath {
    pub time: f64,
    pub x: f32,
    pub y: f32,
    pub team: u8,
    pub victim: u32,
    pub killers: Box<[u32]>,
}

#[derive(Debug, Clone)]
pub struct Game {
    pub id: i64,
    pub map: Option<String>,
    pub mode: Option<i32>,
    pub played_at: Option<DateTime<Utc>>,
    pub winner: Option<u8>,
    pub leads: EarlyLeads,
    pub deaths: Box<[SlimDeath]>,
    /// level lead of team 0: `(start, end, diff)`
    pub level_adv: Box<[(f32, f32, f32)]>,
    /// indices into `Corpus::players`
    pub players: Vec<usize>,
}

#[derive(Debug, Default, Clone)]
pub struct Corpus {
    pub version: Version,
    pub games: HashMap<i64, Game>,
    pub players: Vec<PlayerRow>,
    /// interned toons of `SlimDeath`
    pub toons: Vec<String>,
    toon_ids: HashMap<String, u32>,
}

impl Corpus {
    fn intern(&mut self, toon: &str) -> u32 {
        if let Some(&i) = self.toon_ids.get(toon) {
            return i;
        }
        let i = self.toons.len() as u32;
        self.toons.push(toon.to_string());
        self.toon_ids.insert(toon.to_string(), i);
        i
    }

    fn toon(&self, i: u32) -> String {
        self.toons.get(i as usize).cloned().unwrap_or_default()
    }

    /// The game's facts as `logic` reads them (deaths and level lead only).
    pub fn facts(&self, g: &Game) -> MatchFacts {
        MatchFacts {
            deaths: g
                .deaths
                .iter()
                .map(|d| Death {
                    time: d.time,
                    x: f64::from(d.x),
                    y: f64::from(d.y),
                    team: d.team,
                    victim: self.toon(d.victim),
                    victim_hero: String::new(),
                    killers: d.killers.iter().map(|&k| self.toon(k)).collect(),
                })
                .collect(),
            level_adv: g
                .level_adv
                .iter()
                .map(|&(a, b, d)| (f64::from(a), f64::from(b), f64::from(d)))
                .collect(),
            ..Default::default()
        }
    }
}

fn team(v: Option<i32>) -> Option<u8> {
    v.and_then(|t| u8::try_from(t).ok()).filter(|t| *t < 2)
}

async fn version(db: &PgPool) -> Result<Version, sqlx::Error> {
    let (count, max_id, parser_sum): (i64, i64, i64) = sqlx::query_as(
        "SELECT count(*), COALESCE(max(id), 0), COALESCE(sum(parser_version), 0)::bigint FROM matches",
    )
    .fetch_one(db)
    .await?;
    Ok(Version {
        count,
        max_id,
        parser_sum,
    })
}

type PRow = (
    i64,
    String,
    Option<String>,
    Option<String>,
    Option<i32>,
    Option<bool>,
    Vec<Option<f64>>,
);
type GRow = (
    i64,
    Option<String>,
    Option<i32>,
    Option<DateTime<Utc>>,
    Option<i32>,
    Option<i32>,
    Option<i32>,
    Option<J>,
    Option<J>,
    Option<J>,
);

/// Games read per round trip: bounds the JSON held in memory while loading. Measured on the box's 3,480 games
/// (server RSS after the load, 11 MB without the corpus): all at once ~875 MB, 250 → 216 MB, 50 → 76 MB,
/// 20 → 55 MB, load time ~3.3 s in every case.
const BATCH: i64 = 20;

/// Loads the games with `id > after` into `c`, `BATCH` games at a time.
async fn load_after(db: &PgPool, c: &mut Corpus, after: i64) -> Result<(), sqlx::Error> {
    let mut cursor = after;
    loop {
        let grows: Vec<GRow> = sqlx::query_as(
            "SELECT id, map, mode, played_at, winner, first_fort, parser_version,
                    data->'takedowns', data->'levelTimes', data->'levelAdvTimeline'
             FROM matches WHERE id > $1 ORDER BY id LIMIT $2",
        )
        .bind(cursor)
        .bind(BATCH)
        .fetch_all(db)
        .await?;
        let Some(last) = grows.last().map(|g| g.0) else {
            return Ok(());
        };
        let ids: Vec<i64> = grows.iter().map(|g| g.0).collect();
        let prows: Vec<PRow> = sqlx::query_as(
            // MATERIALIZED: `gameStats` is cut out of the big `data` once per row; inlined, Postgres would
            // detoast `data` once per stat read (7 s instead of 1.4 s on the box's corpus).
            "WITH g AS MATERIALIZED (
               SELECT id, data->'gameStats' AS gs FROM match_players WHERE match_id = ANY($1))
             SELECT mp.match_id, mp.toon_handle, mp.name, mp.hero, mp.team, mp.win,
                    ARRAY[mp.kills::float8, mp.takedowns::float8, (gs->>'Assists')::float8, mp.deaths::float8,
                          mp.hero_damage::float8, (gs->>'SiegeDamage')::float8, mp.healing::float8,
                          (gs->>'SelfHealing')::float8, (gs->>'DamageTaken')::float8, mp.experience::float8,
                          (gs->>'MercCampCaptures')::float8, (gs->>'TimeCCdEnemyHeroes')::float8]
             FROM match_players mp JOIN g ON g.id = mp.id
             ORDER BY mp.match_id, mp.team, mp.id",
        )
        .bind(&ids)
        .fetch_all(db)
        .await?;
        add(c, prows, grows);
        cursor = last;
    }
}

fn add(c: &mut Corpus, prows: Vec<PRow>, grows: Vec<GRow>) {
    let mut by_match: HashMap<i64, Vec<usize>> = HashMap::new();
    // a game projected while we were loading may already be here: never load it twice
    for (match_id, toon, name, hero, t, win, s) in
        prows.into_iter().filter(|r| !c.games.contains_key(&r.0))
    {
        let mut stats = [None; N_STATS];
        for (i, v) in s.into_iter().take(N_STATS).enumerate() {
            stats[i] = v;
        }
        by_match.entry(match_id).or_default().push(c.players.len());
        c.players.push(PlayerRow {
            match_id,
            toon,
            name,
            hero,
            team: team(t),
            win: win.unwrap_or(false),
            stats,
        });
    }
    for (id, map, mode, played_at, winner, fort, _pv, tds, lt, adv) in grows {
        if c.games.contains_key(&id) {
            continue;
        }
        let idx = by_match.remove(&id).unwrap_or_default();
        let teams: HashMap<String, u8> = idx
            .iter()
            .filter_map(|&i| Some((c.players[i].toon.clone(), c.players[i].team?)))
            .collect();
        let m = json!({
            "takedowns": tds.unwrap_or(J::Null),
            "levelTimes": lt.unwrap_or(J::Null),
            "levelAdvTimeline": adv.unwrap_or(J::Null),
        });
        let facts = MatchFacts::from_match(&m, &teams);
        drop(m);
        let deaths = facts
            .deaths
            .iter()
            .map(|d| SlimDeath {
                time: d.time,
                x: d.x as f32,
                y: d.y as f32,
                team: d.team,
                victim: c.intern(&d.victim),
                killers: d.killers.iter().map(|k| c.intern(k)).collect(),
            })
            .collect();
        c.games.insert(
            id,
            Game {
                id,
                map,
                mode,
                played_at,
                winner: team(winner),
                leads: logic::early_leads(&facts, team(fort)),
                deaths,
                level_adv: facts
                    .level_adv
                    .iter()
                    .map(|&(a, b, d)| (a as f32, b as f32, d as f32))
                    .collect(),
                players: idx,
            },
        );
    }
}

fn slot() -> &'static Mutex<Arc<Corpus>> {
    static S: OnceLock<Mutex<Arc<Corpus>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Arc::new(Corpus::default())))
}

/// The corpus, up to date with the database.
pub async fn corpus(db: &PgPool) -> Result<Arc<Corpus>, sqlx::Error> {
    let mut guard = slot().lock().await;
    let v = version(db).await?;
    if guard.version == v {
        return Ok(guard.clone());
    }
    let old = guard.version;
    let mut next = Corpus::default();
    let mut incremental = false;
    if old.count > 0 && v.max_id > old.max_id && v.count > old.count {
        // only new games? check their count and parser versions add up
        let (n, pv): (i64, i64) = sqlx::query_as(
            "SELECT count(*), COALESCE(sum(parser_version), 0)::bigint FROM matches WHERE id > $1",
        )
        .bind(old.max_id)
        .fetch_one(db)
        .await?;
        if old.count + n == v.count && old.parser_sum + pv == v.parser_sum {
            next = Corpus::clone(&guard);
            load_after(db, &mut next, old.max_id).await?;
            incremental = true;
        }
    }
    if !incremental {
        load_after(db, &mut next, i64::MIN).await?;
    }
    next.version = v;
    tracing::debug!(
        games = next.games.len(),
        incremental,
        "insights corpus refreshed"
    );
    *guard = Arc::new(next);
    Ok(guard.clone())
}

/// Loads the corpus in the background at startup, so the first visit of Records or Maps does not wait for it.
pub fn warm_up(db: PgPool) {
    tokio::spawn(async move {
        let t = std::time::Instant::now();
        match corpus(&db).await {
            Ok(c) => tracing::info!(
                games = c.games.len(),
                ms = t.elapsed().as_millis() as u64,
                "insights corpus loaded"
            ),
            Err(e) => tracing::warn!("insights corpus warm-up: {e}"),
        }
    });
}
