//! In-memory corpus for the insights that scan every game (records, early leads, personal insights).
//!
//! Reading a stat out of `match_players.data` detoasts the whole JSON (~8 kB a row): one pass over the box's
//! 35k rows takes ~1 s, so computing records per request took 12 s. The corpus extracts what those endpoints
//! need once and keeps it in memory. Each request first reads a cheap version of `matches` (count, max id, sum
//! of parser versions; no JSON). When it changed, the corpus is diffed against `matches` by `(id,
//! parser_version)`: games gone or re-processed are dropped, missing ones are loaded. Out-of-order ids,
//! deletions and re-uploads are all handled the same way, and only the changed games are read. Games are
//! shared (`Arc`) between successive corpora, so a refresh copies pointers, not games. Nothing is written.

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
    pub toon: String,
    pub name: Option<String>,
    pub hero: Option<String>,
    pub team: Option<u8>,
    pub win: bool,
    pub stats: [Option<f64>; N_STATS],
    /// first end-of-game award, an index into `Corpus::awards`
    pub award: Option<u16>,
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
    pub parser_version: i32,
    pub map: Option<String>,
    pub mode: Option<i32>,
    pub played_at: Option<DateTime<Utc>>,
    pub winner: Option<u8>,
    /// seconds
    pub length: Option<f64>,
    pub leads: EarlyLeads,
    pub deaths: Box<[SlimDeath]>,
    /// level lead of team 0: `(start, end, diff)`
    pub level_adv: Box<[(f32, f32, f32)]>,
    pub players: Vec<PlayerRow>,
}

#[derive(Debug, Default, Clone)]
pub struct Corpus {
    pub version: Version,
    pub games: HashMap<i64, Arc<Game>>,
    /// interned toons of `SlimDeath` (append-only)
    pub toons: Vec<String>,
    toon_ids: HashMap<String, u32>,
    /// interned award ids of `PlayerRow::award` (append-only, a few dozen)
    pub awards: Vec<String>,
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

    fn intern_award(&mut self, award: &str) -> Option<u16> {
        if let Some(i) = self.awards.iter().position(|a| a == award) {
            return u16::try_from(i).ok();
        }
        let i = u16::try_from(self.awards.len()).ok()?;
        self.awards.push(award.to_string());
        Some(i)
    }

    /// The raw award id of a row (`EndOfMatchAwardMVPBoolean`…).
    pub fn award(&self, p: &PlayerRow) -> Option<&str> {
        p.award
            .and_then(|i| self.awards.get(usize::from(i)))
            .map(String::as_str)
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

    /// Every player row with its game.
    pub fn rows(&self) -> impl Iterator<Item = (&PlayerRow, &Game)> {
        self.games
            .values()
            .flat_map(|g| g.players.iter().map(move |p| (p, g.as_ref())))
    }
}

/// Team index 0/1, anything else = unknown.
pub fn team(v: Option<i32>) -> Option<u8> {
    v.and_then(|t| u8::try_from(t).ok()).filter(|t| *t < 2)
}

#[cfg(test)]
tokio::task_local! {
    /// Test seam: a task scoped with a barrier waits on it at the top of `version()`, so a test can
    /// prove two cache hits are inside the version check at the same time (not serialised).
    /// One-shot: only the first `version()` of the task waits.
    static VERSION_GATE: std::cell::Cell<Option<Arc<tokio::sync::Barrier>>>;
    /// Test seam: a task scoped with `(entered, release)` signals `entered` at the top of `load()`
    /// and waits for `release`, once per task (a retried reload runs unblocked).
    static LOAD_GATE: std::cell::Cell<Option<TestGate>>;
    /// Same seam between the guarded snapshot and the version recheck.
    static RECHECK_GATE: std::cell::Cell<Option<TestGate>>;
}

#[cfg(test)]
type TestGate = (Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>);

#[cfg(test)]
async fn pause_at(gate: &'static tokio::task::LocalKey<std::cell::Cell<Option<TestGate>>>) {
    if let Ok(Some((entered, release))) = gate.try_with(std::cell::Cell::take) {
        entered.notify_one();
        release.notified().await;
    }
}

async fn version(db: &PgPool) -> Result<Version, sqlx::Error> {
    #[cfg(test)]
    if let Ok(Some(b)) = VERSION_GATE.try_with(std::cell::Cell::take) {
        b.wait().await;
    }
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
    Option<String>,
);
type GRow = (
    i64,
    Option<String>,
    Option<i32>,
    Option<DateTime<Utc>>,
    Option<i32>,
    Option<i32>,
    i32,
    Option<f64>,
    Option<J>,
    Option<J>,
    Option<J>,
);

/// Games read per round trip: bounds the JSON held in memory while loading. Measured on the box's 3,480 games
/// (server RSS after the load, 11 MB without the corpus): all at once ~875 MB, 250 → 216 MB, 50 → 76 MB,
/// 20 → 55 MB, load time ~3.3 s in every case.
const BATCH: usize = 20;

/// Loads the games `ids` into `c`, `BATCH` games at a time.
async fn load(db: &PgPool, c: &mut Corpus, ids: &[i64]) -> Result<(), sqlx::Error> {
    #[cfg(test)]
    pause_at(&LOAD_GATE).await;
    for chunk in ids.chunks(BATCH) {
        let grows: Vec<GRow> = sqlx::query_as(
            "SELECT id, map, mode, played_at, winner, first_fort, COALESCE(parser_version, 0), length,
                    data->'takedowns', data->'levelTimes', data->'levelAdvTimeline'
             FROM matches WHERE id = ANY($1)",
        )
        .bind(chunk)
        .fetch_all(db)
        .await?;
        let prows: Vec<PRow> = sqlx::query_as(
            // MATERIALIZED: `gameStats` is cut out of the big `data` once per row; inlined, Postgres would
            // detoast `data` once per stat read (7 s instead of 1.4 s on the box's corpus).
            "WITH g AS MATERIALIZED (
               SELECT id, data->'gameStats' AS gs FROM match_players WHERE match_id = ANY($1))
             SELECT mp.match_id, mp.toon_handle, mp.name, mp.hero, mp.team, mp.win,
                    ARRAY[mp.kills::float8, mp.takedowns::float8, (gs->>'Assists')::float8, mp.deaths::float8,
                          mp.hero_damage::float8, (gs->>'SiegeDamage')::float8, mp.healing::float8,
                          (gs->>'SelfHealing')::float8, (gs->>'DamageTaken')::float8, mp.experience::float8,
                          (gs->>'MercCampCaptures')::float8, (gs->>'TimeCCdEnemyHeroes')::float8],
                    gs #>> '{awards,0}'
             FROM match_players mp JOIN g ON g.id = mp.id
             ORDER BY mp.match_id, mp.team, mp.id",
        )
        .bind(chunk)
        .fetch_all(db)
        .await?;
        add(c, prows, grows);
    }
    Ok(())
}

fn add(c: &mut Corpus, prows: Vec<PRow>, grows: Vec<GRow>) {
    let mut by_match: HashMap<i64, Vec<PlayerRow>> = HashMap::new();
    for (match_id, toon, name, hero, t, win, s, award) in prows {
        let mut stats = [None; N_STATS];
        for (i, v) in s.into_iter().take(N_STATS).enumerate() {
            stats[i] = v;
        }
        by_match.entry(match_id).or_default().push(PlayerRow {
            toon,
            name,
            hero,
            team: team(t),
            win: win.unwrap_or(false),
            stats,
            award: award.as_deref().and_then(|a| c.intern_award(a)),
        });
    }
    for (id, map, mode, played_at, winner, fort, parser_version, length, tds, lt, adv) in grows {
        let players = by_match.remove(&id).unwrap_or_default();
        let teams: HashMap<String, u8> = players
            .iter()
            .filter_map(|p| Some((p.toon.clone(), p.team?)))
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
            Arc::new(Game {
                id,
                parser_version,
                map,
                mode,
                played_at,
                winner: team(winner),
                length,
                leads: logic::early_leads(&facts, team(fort)),
                deaths,
                level_adv: facts
                    .level_adv
                    .iter()
                    .map(|&(a, b, d)| (a as f32, b as f32, d as f32))
                    .collect(),
                players,
            }),
        );
    }
}

/// What to change in `have` to match `want` (`id → parser_version`): ids to drop (gone or re-processed),
/// ids to load (missing or re-processed, sorted).
pub fn diff(have: &HashMap<i64, i32>, want: &HashMap<i64, i32>) -> (Vec<i64>, Vec<i64>) {
    let gone: Vec<i64> = have
        .iter()
        .filter(|(id, pv)| want.get(id) != Some(pv))
        .map(|(id, _)| *id)
        .collect();
    let mut load: Vec<i64> = want
        .iter()
        .filter(|(id, pv)| have.get(id) != Some(pv))
        .map(|(id, _)| *id)
        .collect();
    load.sort_unstable();
    (gone, load)
}

/// The published corpus and its generation. Only ever locked briefly (clone or swap an `Arc`), never
/// across a database round trip: cache hits run concurrently.
#[derive(Default)]
struct Slot {
    generation: u64,
    corpus: Arc<Corpus>,
}

#[derive(Default)]
struct Cache {
    slot: std::sync::Mutex<Slot>,
    /// Held across the version recheck, id scan and load, so reloads run one at a time.
    refresh: Mutex<()>,
}

fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(Cache::default)
}

impl Cache {
    fn slot(&self) -> std::sync::MutexGuard<'_, Slot> {
        // A panic elsewhere while swapping an Arc leaves this value consistent.
        self.slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn invalidate(&self) {
        let mut s = self.slot();
        s.generation += 1;
        s.corpus = Arc::new(Corpus::default());
    }

    async fn corpus(&self, db: &PgPool) -> Result<Arc<Corpus>, sqlx::Error> {
        let v = version(db).await?;
        let cur = self.slot().corpus.clone();
        if cur.version == v {
            return Ok(cur);
        }
        loop {
            if let Some(next) = self.refresh(db).await? {
                return Ok(next);
            }
        }
    }

    /// `None` means the snapshot was invalidated while we were using it. Retry with a fresh one.
    async fn refresh(&self, db: &PgPool) -> Result<Option<Arc<Corpus>>, sqlx::Error> {
        let _reload = self.refresh.lock().await;
        // Another reload may have finished while we waited: recheck against the database now.
        let (generation, cur) = {
            let s = self.slot();
            (s.generation, s.corpus.clone())
        };
        #[cfg(test)]
        pause_at(&RECHECK_GATE).await;
        let v = version(db).await?;
        let next = if cur.version == v {
            cur
        } else {
            reload(db, &cur, v).await?
        };
        let mut s = self.slot();
        if s.generation != generation {
            return Ok(None);
        }
        s.corpus = next.clone();
        Ok(Some(next))
    }
}

/// Reference hero repairs preserve match IDs and parser versions, so those edits are not
/// detectable by the normal version check. Bump the generation without waiting for a reload,
/// so a reload that may have read pre-repair games cannot publish or return them.
pub(crate) async fn invalidate() {
    cache().invalidate();
}

/// Returns the shared corpus; an invalidated refresh retries against the current generation.
pub async fn corpus(db: &PgPool) -> Result<Arc<Corpus>, sqlx::Error> {
    cache().corpus(db).await
}

async fn reload(db: &PgPool, cur: &Corpus, v: Version) -> Result<Arc<Corpus>, sqlx::Error> {
    // the ids read here are exactly what the new corpus holds, whatever is committed meanwhile
    let want: HashMap<i64, i32> =
        sqlx::query_as::<_, (i64, i32)>("SELECT id, COALESCE(parser_version, 0) FROM matches")
            .fetch_all(db)
            .await?
            .into_iter()
            .collect();
    let have: HashMap<i64, i32> = cur
        .games
        .iter()
        .map(|(id, g)| (*id, g.parser_version))
        .collect();
    let (gone, missing) = diff(&have, &want);
    let mut next = cur.clone();
    for id in &gone {
        next.games.remove(id);
    }
    load(db, &mut next, &missing).await?;
    // a game deleted between the id list and its load is simply absent from `load`'s result
    next.version = v;
    tracing::debug!(
        games = next.games.len(),
        dropped = gone.len(),
        loaded = missing.len(),
        "insights corpus refreshed"
    );
    Ok(Arc::new(next))
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::players::testutil::{insert_match, row, uid};

    #[test]
    fn diff_drops_gone_and_reprocessed_games_and_loads_missing_ones() {
        let have: HashMap<i64, i32> = [(1, 5), (2, 5), (3, 5)].into_iter().collect();
        // 2 deleted, 3 re-processed, 4 and 9 new (out of order is fine)
        let want: HashMap<i64, i32> = [(1, 5), (3, 6), (9, 5), (4, 5)].into_iter().collect();
        let (mut gone, load) = diff(&have, &want);
        gone.sort_unstable();
        assert_eq!(gone, vec![2, 3]);
        assert_eq!(load, vec![3, 4, 9]);
        assert_eq!(diff(&want, &want), (vec![], vec![]));
    }

    #[test]
    fn team_index_is_zero_or_one() {
        assert_eq!(team(Some(1)), Some(1));
        assert_eq!(team(Some(2)), None);
        assert_eq!(team(Some(-1)), None);
        assert_eq!(team(None), None);
    }

    /// M4: two requests that both hit an up-to-date corpus must be able to run their version
    /// check at the same time. Each one waits at a 2-party barrier inside `version()`: if the
    /// corpus lock is held across that query, the second never gets in and both stall.
    #[tokio::test]
    async fn concurrent_cache_hits_are_not_serialised() {
        let Some(state) = crate::api_tests::test_state(None).await else {
            return;
        };
        corpus(&state.db).await.unwrap();
        let gate = Arc::new(tokio::sync::Barrier::new(2));
        let hit = || {
            let db = state.db.clone();
            tokio::spawn(VERSION_GATE.scope(Some(gate.clone()).into(), async move {
                corpus(&db).await.map(|_| ())
            }))
        };
        let (a, b) = (hit(), hit());
        let (abort_a, abort_b) = (a.abort_handle(), b.abort_handle());
        let both = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            (a.await, b.await)
        })
        .await;
        // never leave a stalled task holding the process-wide corpus for the other tests
        abort_a.abort();
        abort_b.abort();
        let (a, b) = both.expect("two cache hits were serialised behind the corpus lock");
        a.unwrap().unwrap();
        b.unwrap().unwrap();
    }

    /// The cold reload must run even on a fresh database and must not block invalidation.
    #[tokio::test]
    async fn invalidate_during_a_reload_is_neither_blocked_nor_lost() {
        let Some((db, name)) = crate::refdata::tests::isolated_db().await else {
            return;
        };
        let game = insert_match(&db, None, &[row(&uid(), "probe", 0, "Valla")]).await;
        let cache = Cache::default();
        let loaded = assert_repaired_after_invalidation(&cache, &db, game).await;
        assert_eq!(loaded.games.len(), 1);
        crate::refdata::tests::drop_db(db, &name).await;
    }

    /// An incremental reload has already copied the cached game when the hero repair lands.
    /// The original caller (not just a later request) must see the corrected hero.
    #[tokio::test]
    async fn invalidated_inflight_caller_gets_the_repaired_hero() {
        let Some((db, name)) = crate::refdata::tests::isolated_db().await else {
            return;
        };
        let game = insert_match(&db, None, &[row(&uid(), "probe", 0, "Valla")]).await;
        let cache = Cache::default();
        let before = cache.corpus(&db).await.unwrap();
        assert_eq!(
            before.games[&game].players[0].hero.as_deref(),
            Some("Valla")
        );
        insert_match(&db, None, &[]).await;
        let loaded = assert_repaired_after_invalidation(&cache, &db, game).await;
        assert_eq!(loaded.games.len(), 2);
        crate::refdata::tests::drop_db(db, &name).await;
    }

    async fn assert_repaired_after_invalidation(
        cache: &Cache,
        db: &PgPool,
        game: i64,
    ) -> Arc<Corpus> {
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let reload = LOAD_GATE.scope(
            Some((entered.clone(), release.clone())).into(),
            cache.corpus(db),
        );
        let invalidate = async {
            entered.notified().await;
            sqlx::query("UPDATE match_players SET hero = 'Uther' WHERE match_id = $1")
                .bind(game)
                .execute(db)
                .await
                .unwrap();
            cache.invalidate();
            release.notify_one();
        };
        let (loaded, ()) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            tokio::join!(reload, invalidate)
        })
        .await
        .expect("reload never reached the gate, or invalidation waited for the reload");
        let loaded = loaded.unwrap();
        assert_eq!(
            loaded.games[&game].players[0].hero.as_deref(),
            Some("Uther")
        );
        assert!(Arc::ptr_eq(&loaded, &cache.slot().corpus));
        loaded
    }

    #[tokio::test]
    async fn guarded_version_match_checks_generation_before_returning() {
        let Some((db, name)) = crate::refdata::tests::isolated_db().await else {
            return;
        };
        let cache = Cache::default();
        insert_match(&db, None, &[]).await;
        cache.corpus(&db).await.unwrap();
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        // The guarded recheck sees a matching version, as when another reload just finished.
        let recheck = RECHECK_GATE.scope(
            Some((entered.clone(), release.clone())).into(),
            cache.refresh(&db),
        );
        let invalidate = async {
            entered.notified().await;
            cache.invalidate();
            release.notify_one();
        };
        let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            tokio::join!(recheck, invalidate)
        })
        .await
        .expect("guarded version recheck stalled");
        assert!(
            result.unwrap().is_none(),
            "recheck returned an invalidated corpus"
        );
        crate::refdata::tests::drop_db(db, &name).await;
    }
}
