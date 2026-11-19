//! Session review (spec `docs/specs/2026-11-19-session-review.md`): the operator's games split into play sessions
//! (a gap of more than 3 h between two game starts), and one session against the operator's own baseline (the 90
//! days before it, in the game modes played in the session). Pure logic over the insights corpus; nothing is written.

use super::cache::{Corpus, Game, PlayerRow};
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};

/// A new session starts when two consecutive game starts are more than this apart.
pub const GAP_S: i64 = 3 * 3600;
/// Baseline window before the session start.
pub const BASELINE_DAYS: i64 = 90;
/// A hero (or map) baseline needs this many games; under it the overall average is used and the row is flagged.
pub const MIN_HERO_GAMES: u32 = 5;
/// Under this many baseline games the whole baseline is flagged (same threshold as the personal insights).
pub const SMALL_BASELINE: u32 = super::api::SMALL_SAMPLE as u32;

/// The 7 compared stats, in `MyGame::stats` order.
pub const STATS: [&str; 7] = [
    "kills",
    "deaths",
    "assists",
    "hero_damage",
    "siege_damage",
    "healing",
    "experience",
];
/// Index of `deaths` in `STATS`: the only stat where less is better.
const DEATHS: usize = 1;

/// One game seen from the operator's row.
#[derive(Debug, Clone, PartialEq)]
pub struct MyGame {
    pub id: i64,
    pub played_at: DateTime<Utc>,
    pub map: Option<String>,
    pub mode: Option<i32>,
    pub hero: Option<String>,
    pub win: bool,
    /// seconds
    pub length: Option<f64>,
    pub award: Option<String>,
    pub stats: [Option<f64>; 7],
}

impl MyGame {
    fn stat(&self, i: usize) -> f64 {
        self.stats[i].unwrap_or(0.0)
    }

    /// (kills + assists) / max(deaths, 1)
    pub fn kda(&self) -> f64 {
        (self.stat(0) + self.stat(2)) / self.stat(DEATHS).max(1.0)
    }
}

/// Whose games: every operator account, or one (`name` or `toon:<handle>` of a verified folder).
pub struct Perspective<'a> {
    /// `operator_names`, lower case
    pub ops: &'a [String],
    /// verified toons (standalone folders)
    pub toons: &'a [String],
    pub account: Option<&'a str>,
}

impl Perspective<'_> {
    /// Whether `account` names one of the operator's accounts (always true without one).
    pub fn known(&self) -> bool {
        match self.account {
            None => true,
            Some(a) => match a.strip_prefix("toon:") {
                Some(t) => self.toons.iter().any(|x| x == t),
                None => self.ops.contains(&a.to_lowercase()),
            },
        }
    }

    fn matches(&self, p: &PlayerRow) -> bool {
        let name = || p.name.as_deref().map(str::to_lowercase);
        match self.account {
            None => self.toons.contains(&p.toon) || name().is_some_and(|n| self.ops.contains(&n)),
            Some(a) => match a.strip_prefix("toon:") {
                Some(t) => p.toon == t && self.toons.contains(&p.toon),
                None => name().as_deref() == Some(a.to_lowercase().as_str()),
            },
        }
    }

    /// The operator's row of `g`, chosen like `/api/matches`: a verified toon first, then by team and row id
    /// (the corpus keeps rows in that order).
    pub fn row<'g>(&self, g: &'g Game) -> Option<&'g PlayerRow> {
        let mut rows = g.players.iter().filter(|p| self.matches(p));
        let first = rows.next()?;
        if self.toons.contains(&first.toon) {
            return Some(first);
        }
        Some(rows.find(|p| self.toons.contains(&p.toon)).unwrap_or(first))
    }
}

/// The operator's dated games in `corpus`, oldest first (ties by id).
pub fn my_games(corpus: &Corpus, who: &Perspective) -> Vec<MyGame> {
    let mut out: Vec<MyGame> = corpus
        .games
        .values()
        .filter_map(|g| {
            let played_at = g.played_at?;
            let p = who.row(g)?;
            let s = |i: usize| p.stats[i];
            // assists as the pages show them (takedowns − kills), the stored count otherwise
            let assists = match (s(1), s(0)) {
                (Some(t), Some(k)) => Some((t - k).max(0.0)),
                _ => s(2),
            };
            Some(MyGame {
                id: g.id,
                played_at,
                map: g.map.clone(),
                mode: g.mode,
                hero: p.hero.clone(),
                win: p.win,
                length: g.length,
                award: corpus.award(p).map(str::to_string),
                // corpus order: kills, takedowns, assists, deaths, hero_damage, siege_damage, healing, …, experience
                stats: [s(0), s(3), assists, s(4), s(5), s(6), s(9)],
            })
        })
        .collect();
    out.sort_by(|a, b| a.played_at.cmp(&b.played_at).then(a.id.cmp(&b.id)));
    out
}

/// Sessions of `games` (oldest first) as index ranges, oldest first: a gap of more than `GAP_S` seconds between
/// two consecutive starts opens a new one.
pub fn split(games: &[MyGame]) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    for i in 1..games.len() {
        if games[i].played_at - games[i - 1].played_at > Duration::seconds(GAP_S) {
            out.push(start..i);
            start = i;
        }
    }
    if !games.is_empty() {
        out.push(start..games.len());
    }
    out
}

/// End of a session: the last start plus the last game's length.
fn end_of(games: &[MyGame]) -> Option<DateTime<Utc>> {
    let last = games.last()?;
    Some(last.played_at + ms(last.length.unwrap_or(0.0)))
}

fn ms(seconds: f64) -> Duration {
    Duration::milliseconds((seconds * 1000.0).round() as i64)
}

#[derive(Debug, Serialize, PartialEq)]
pub struct SessionItem {
    pub id: i64,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub games: usize,
    pub wins: usize,
}

/// The `limit` most recent sessions, newest first.
pub fn list(games: &[MyGame], limit: usize) -> Vec<SessionItem> {
    split(games)
        .into_iter()
        .rev()
        .take(limit)
        .filter_map(|r| {
            let s = &games[r];
            Some(SessionItem {
                id: s.first()?.id,
                start: s.first()?.played_at,
                end: end_of(s)?,
                games: s.len(),
                wins: s.iter().filter(|g| g.win).count(),
            })
        })
        .collect()
}

/// Per-game average of each stat over `games` (games without the stat are skipped); `None` without any value.
pub fn averages<'a>(games: impl Iterator<Item = &'a MyGame> + Clone) -> [Option<f64>; 7] {
    std::array::from_fn(|i| {
        let (n, sum) = games
            .clone()
            .filter_map(|g| g.stats[i])
            .fold((0u32, 0.0), |(n, s), v| (n + 1, s + v));
        (n > 0).then(|| sum / f64::from(n))
    })
}

/// `value − average` for each stat both sides know.
pub fn deltas(g: &MyGame, avg: &[Option<f64>; 7]) -> [Option<f64>; 7] {
    std::array::from_fn(|i| Some(g.stats[i]? - avg[i]?))
}

/// Number of stats better than the baseline (above it; below it for deaths).
pub fn score(d: &[Option<f64>; 7]) -> u32 {
    d.iter()
        .enumerate()
        .filter(|(i, v)| v.is_some_and(|v| if *i == DEATHS { v < 0.0 } else { v > 0.0 }))
        .count() as u32
}

/// Best and worst game (indices into `games`) by `scores` (ties: KDA ratio); `None` for fewer than 2 games.
pub fn best_worst(games: &[MyGame], scores: &[u32]) -> Option<(usize, usize)> {
    if games.len() < 2 {
        return None;
    }
    let key = |i: &usize| (scores[*i], games[*i].kda());
    let cmp = |a: &usize, b: &usize| {
        let (ka, kb) = (key(a), key(b));
        ka.0.cmp(&kb.0).then(ka.1.total_cmp(&kb.1))
    };
    let best = (0..games.len()).max_by(cmp)?;
    let worst = (0..games.len()).min_by(cmp)?;
    Some((best, worst))
}

#[derive(Debug, Serialize, PartialEq, Eq, Default)]
pub struct Streaks {
    /// longest run of wins
    pub best_win: u32,
    /// longest run of losses
    pub worst_loss: u32,
    /// run at the end: > 0 wins, < 0 losses
    pub current: i32,
}

pub fn streaks(results: &[bool]) -> Streaks {
    let mut s = Streaks::default();
    let mut run = 0i32;
    for &w in results {
        run = match (w, run) {
            (true, r) if r > 0 => r + 1,
            (true, _) => 1,
            (false, r) if r < 0 => r - 1,
            (false, _) => -1,
        };
        s.best_win = s.best_win.max(run.max(0) as u32);
        s.worst_loss = s.worst_loss.max((-run).max(0) as u32);
    }
    s.current = run;
    s
}

#[derive(Debug, Serialize)]
pub struct ReviewGame {
    pub match_id: i64,
    pub played_at: DateTime<Utc>,
    pub hero: Option<String>,
    pub map: Option<String>,
    pub mode: Option<i32>,
    pub win: bool,
    pub length: Option<f64>,
    pub kills: Option<f64>,
    pub deaths: Option<f64>,
    pub assists: Option<f64>,
    pub award: Option<String>,
    pub deltas: BTreeMap<&'static str, f64>,
    /// "hero" (≥ 5 baseline games on the hero) or "overall"
    pub baseline: &'static str,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Row {
    pub name: String,
    pub games: u32,
    pub wins: u32,
    pub base_games: u32,
    pub base_wins: u32,
    pub small_sample: bool,
}

#[derive(Debug, Serialize)]
pub struct Baseline {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    /// game modes of the session (the baseline only counts those)
    pub modes: Vec<Option<i32>>,
    pub games: u32,
    pub wins: u32,
    pub averages: BTreeMap<&'static str, f64>,
    pub small_sample: bool,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Record {
    pub wins: usize,
    pub losses: usize,
}

#[derive(Debug, Serialize)]
pub struct Review {
    pub id: i64,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub span_s: f64,
    pub played_s: f64,
    pub games: Vec<ReviewGame>,
    pub record: Record,
    pub streaks: Streaks,
    pub best: Option<i64>,
    pub worst: Option<i64>,
    pub heroes: Vec<Row>,
    pub maps: Vec<Row>,
    pub baseline: Baseline,
    /// previous (older) session id
    pub prev: Option<i64>,
    /// next (newer) session id
    pub next: Option<i64>,
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn by_stat(v: &[Option<f64>; 7]) -> BTreeMap<&'static str, f64> {
    STATS
        .iter()
        .zip(v)
        .filter_map(|(k, v)| Some((*k, round1((*v)?))))
        .collect()
}

/// Session rows per hero or map (`key`) with that key's baseline record; most played first, then by name.
fn rows(
    session: &[MyGame],
    base: &[&MyGame],
    key: impl Fn(&MyGame) -> Option<&String>,
) -> Vec<Row> {
    let mut out: Vec<Row> = Vec::new();
    for g in session {
        let Some(k) = key(g) else { continue };
        let i = match out.iter().position(|r| &r.name == k) {
            Some(i) => i,
            None => {
                let (base_games, base_wins) = base
                    .iter()
                    .filter(|b| key(b) == Some(k))
                    .fold((0, 0), |(n, w), b| (n + 1, w + u32::from(b.win)));
                out.push(Row {
                    name: k.clone(),
                    games: 0,
                    wins: 0,
                    base_games,
                    base_wins,
                    small_sample: base_games < MIN_HERO_GAMES,
                });
                out.len() - 1
            }
        };
        out[i].games += 1;
        out[i].wins += u32::from(g.win);
    }
    out.sort_by(|a, b| b.games.cmp(&a.games).then(a.name.cmp(&b.name)));
    out
}

/// The review of the session starting with game `id`, or `None` if no session starts with it.
pub fn review(games: &[MyGame], id: i64) -> Option<Review> {
    let sessions = split(games);
    let k = sessions.iter().position(|r| games[r.start].id == id)?;
    let session = &games[sessions[k].clone()];
    let first = session.first()?;
    let start = first.played_at;
    let end = end_of(session)?;
    let from = start - Duration::days(BASELINE_DAYS);
    let modes: Vec<Option<i32>> = {
        let set: HashSet<Option<i32>> = session.iter().map(|g| g.mode).collect();
        let mut v: Vec<_> = set.into_iter().collect();
        v.sort_unstable();
        v
    };
    let base: Vec<&MyGame> = games
        .iter()
        .filter(|g| g.played_at >= from && g.played_at < start && modes.contains(&g.mode))
        .collect();
    let overall = averages(base.iter().copied());
    let mut out_games = Vec::with_capacity(session.len());
    let mut scores = Vec::with_capacity(session.len());
    for g in session {
        let hero_base: Vec<&MyGame> = base
            .iter()
            .copied()
            .filter(|b| g.hero.is_some() && b.hero == g.hero)
            .collect();
        let (avg, which) = if hero_base.len() >= MIN_HERO_GAMES as usize {
            (averages(hero_base.iter().copied()), "hero")
        } else {
            (overall, "overall")
        };
        let d = deltas(g, &avg);
        scores.push(score(&d));
        out_games.push(ReviewGame {
            match_id: g.id,
            played_at: g.played_at,
            hero: g.hero.clone(),
            map: g.map.clone(),
            mode: g.mode,
            win: g.win,
            length: g.length,
            kills: g.stats[0],
            deaths: g.stats[DEATHS],
            assists: g.stats[2],
            award: g.award.clone(),
            deltas: by_stat(&d),
            baseline: which,
        });
    }
    let (best, worst) = match best_worst(session, &scores) {
        Some((b, w)) => (Some(session[b].id), Some(session[w].id)),
        None => (None, None),
    };
    let wins = session.iter().filter(|g| g.win).count();
    let base_games = base.len() as u32;
    Some(Review {
        id,
        start,
        end,
        span_s: (end - start).num_milliseconds() as f64 / 1000.0,
        played_s: session.iter().filter_map(|g| g.length).sum(),
        games: out_games,
        record: Record {
            wins,
            losses: session.len() - wins,
        },
        streaks: streaks(&session.iter().map(|g| g.win).collect::<Vec<_>>()),
        best,
        worst,
        heroes: rows(session, &base, |g| g.hero.as_ref()),
        maps: rows(session, &base, |g| g.map.as_ref()),
        baseline: Baseline {
            from,
            to: start,
            modes,
            games: base_games,
            wins: base.iter().filter(|g| g.win).count() as u32,
            averages: by_stat(&overall),
            small_sample: base_games < SMALL_BASELINE,
        },
        prev: k
            .checked_sub(1)
            .and_then(|p| sessions.get(p))
            .map(|r| games[r.start].id),
        next: sessions.get(k + 1).map(|r| games[r.start].id),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(s: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000 + s, 0).unwrap()
    }

    /// Game `id` at `t` seconds, `hero`, mode 1, all 7 stats = `v` (deaths = `d`).
    fn g(id: i64, t: i64, hero: &str, win: bool, v: f64, d: f64) -> MyGame {
        MyGame {
            id,
            played_at: at(t),
            map: Some(format!("Map {}", id % 2)),
            mode: Some(1),
            hero: Some(hero.into()),
            win,
            length: Some(1200.0),
            award: None,
            stats: [
                Some(v),
                Some(d),
                Some(v),
                Some(v),
                Some(v),
                Some(v),
                Some(v),
            ],
        }
    }

    const DAY: i64 = 86_400;

    #[test]
    fn splits_on_gaps_over_three_hours_only() {
        let games = [
            g(1, 0, "A", true, 1.0, 1.0),
            g(2, GAP_S, "A", true, 1.0, 1.0), // exactly 3 h: same session
            g(3, 2 * GAP_S + 1, "A", true, 1.0, 1.0), // 3 h + 1 s: new one
        ];
        assert_eq!(split(&games), vec![0..2, 2..3]);
        assert!(split(&[]).is_empty());
        let l = list(&games, 20);
        assert_eq!(
            l.iter().map(|s| s.id).collect::<Vec<_>>(),
            vec![3, 1],
            "newest first"
        );
        assert_eq!(l[1].games, 2);
        assert_eq!(l[1].end, at(GAP_S + 1200));
        assert_eq!(list(&games, 1).len(), 1);
    }

    #[test]
    fn games_without_date_are_ignored() {
        let mut c = Corpus::default();
        let row = |name: &str| PlayerRow {
            toon: format!("t-{name}"),
            name: Some(name.into()),
            hero: Some("Valla".into()),
            team: Some(0),
            win: true,
            stats: [Some(1.0); super::super::cache::N_STATS],
            award: None,
        };
        for (id, date) in [(1, Some(at(0))), (2, None)] {
            c.games.insert(
                id,
                std::sync::Arc::new(Game {
                    id,
                    parser_version: 1,
                    map: None,
                    mode: None,
                    played_at: date,
                    winner: Some(0),
                    length: None,
                    leads: Default::default(),
                    deaths: Box::new([]),
                    level_adv: Box::new([]),
                    players: vec![row("Other"), row("Me")],
                }),
            );
        }
        let ops = vec!["me".to_string()];
        let who = Perspective {
            ops: &ops,
            toons: &[],
            account: None,
        };
        let mine = my_games(&c, &who);
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].id, 1);
        // assists = takedowns − kills
        assert_eq!(mine[0].stats[2], Some(0.0));
    }

    #[test]
    fn perspective_prefers_verified_toons_and_checks_accounts() {
        let ops = vec!["manual".to_string()];
        let toons = vec!["t-v".to_string()];
        let row = |toon: &str, name: &str| PlayerRow {
            toon: toon.into(),
            name: Some(name.into()),
            hero: None,
            team: Some(0),
            win: false,
            stats: [None; super::super::cache::N_STATS],
            award: None,
        };
        let game = Game {
            id: 1,
            parser_version: 1,
            map: None,
            mode: None,
            played_at: None,
            winner: None,
            length: None,
            leads: Default::default(),
            deaths: Box::new([]),
            level_adv: Box::new([]),
            players: vec![row("t-m", "Manual"), row("t-v", "Old")],
        };
        let who = |account| Perspective {
            ops: &ops,
            toons: &toons,
            account,
        };
        assert_eq!(who(None).row(&game).unwrap().toon, "t-v");
        assert_eq!(who(Some("MANUAL")).row(&game).unwrap().toon, "t-m");
        assert_eq!(who(Some("toon:t-v")).row(&game).unwrap().toon, "t-v");
        assert!(who(Some("toon:t-m")).row(&game).is_none());
        assert!(who(Some("manual")).known() && who(Some("toon:t-v")).known());
        assert!(!who(Some("nobody")).known() && !who(Some("toon:t-m")).known());
        assert!(who(None).known());
    }

    #[test]
    fn baseline_excludes_the_session_old_games_and_other_modes() {
        let mut other_mode = g(4, -DAY, "A", true, 100.0, 0.0);
        other_mode.mode = Some(2);
        let games = vec![
            g(1, -91 * DAY, "A", true, 100.0, 0.0), // older than 90 days
            g(2, -10 * DAY, "A", false, 4.0, 2.0),
            g(3, -9 * DAY, "A", true, 6.0, 4.0),
            other_mode,
            g(10, 0, "A", true, 8.0, 1.0),
            g(11, 600, "A", false, 2.0, 5.0),
        ];
        let mut games = games;
        games.sort_by_key(|g| g.played_at);
        let r = review(&games, 10).unwrap();
        assert_eq!(r.baseline.games, 2);
        assert_eq!(r.baseline.wins, 1);
        assert_eq!(r.baseline.modes, vec![Some(1)]);
        assert_eq!(r.baseline.averages["kills"], 5.0);
        assert_eq!(r.baseline.averages["deaths"], 3.0);
        assert!(r.baseline.small_sample);
        assert_eq!(r.baseline.to, at(0));
        assert_eq!(r.baseline.from, at(-90 * DAY));
        // under 5 hero games: compared with the overall average
        assert_eq!(r.games[0].baseline, "overall");
        assert_eq!(r.games[0].deltas["kills"], 3.0);
        assert_eq!(r.games[0].deltas["deaths"], -2.0);
        assert_eq!(r.prev, Some(4), "the previous session, whatever its mode");
        assert_eq!(r.next, None);
        assert!(
            review(&games, 11).is_none(),
            "not the first game of a session"
        );
    }

    #[test]
    fn hero_baseline_with_five_games_else_overall() {
        let mut games: Vec<MyGame> = (0..5)
            .map(|i| g(i, -DAY * (i + 1), "A", true, 10.0, 1.0))
            .chain((5..9).map(|i| g(i, -DAY * (i + 1), "B", false, 2.0, 1.0)))
            .collect();
        games.push(g(20, 0, "A", true, 12.0, 1.0));
        games.push(g(21, 100, "B", false, 4.0, 1.0));
        games.sort_by_key(|g| g.played_at);
        let r = review(&games, 20).unwrap();
        assert_eq!(r.games[0].baseline, "hero");
        assert_eq!(r.games[0].deltas["kills"], 2.0);
        assert_eq!(r.games[1].baseline, "overall");
        // overall = (5×10 + 4×2) / 9
        assert_eq!(r.games[1].deltas["kills"], round1(4.0 - 58.0 / 9.0));
        let a = r.heroes.iter().find(|h| h.name == "A").unwrap();
        assert_eq!((a.games, a.wins, a.base_games, a.base_wins), (1, 1, 5, 5));
        assert!(!a.small_sample);
        let b = r.heroes.iter().find(|h| h.name == "B").unwrap();
        assert!(b.small_sample);
        assert_eq!(r.maps.iter().map(|m| m.games).sum::<u32>(), 2);
    }

    #[test]
    fn best_and_worst_count_stats_above_baseline_with_kda_ties() {
        let avg = [Some(5.0); 7];
        // 6 above + fewer deaths: 7
        let top = g(1, 0, "A", false, 6.0, 4.0);
        assert_eq!(score(&deltas(&top, &avg)), 7);
        // equal is neither above nor below; more deaths is worse
        let even = g(2, 10, "A", true, 5.0, 9.0);
        assert_eq!(score(&deltas(&even, &avg)), 0);
        let games = [top, even.clone(), g(3, 20, "A", true, 5.0, 1.0)];
        // game 3: only deaths better → 1; game 2: 0
        let scores: Vec<u32> = games.iter().map(|x| score(&deltas(x, &avg))).collect();
        assert_eq!(scores, vec![7, 0, 1]);
        assert_eq!(best_worst(&games, &scores), Some((0, 1)));
        // same score: the KDA ratio decides (a loss can be the best game)
        let tie = [g(1, 0, "A", true, 2.0, 2.0), g(2, 1, "A", false, 8.0, 2.0)];
        assert_eq!(best_worst(&tie, &[3, 3]), Some((1, 0)));
        // all equal: still two different games
        let same = [even.clone(), even];
        let (b, w) = best_worst(&same, &[0, 0]).unwrap();
        assert_ne!(b, w);
        assert_eq!(best_worst(&games[..1], &scores[..1]), None);
    }

    #[test]
    fn streaks_longest_and_current() {
        let s = streaks(&[true, true, false, true, true, true, false, false]);
        assert_eq!(
            s,
            Streaks {
                best_win: 3,
                worst_loss: 2,
                current: -2
            }
        );
        assert_eq!(streaks(&[true]).current, 1);
        assert_eq!(streaks(&[]), Streaks::default());
    }

    #[test]
    fn single_game_session_has_no_best_or_worst() {
        let games = [g(1, 0, "A", true, 1.0, 1.0)];
        let r = review(&games, 1).unwrap();
        assert_eq!((r.best, r.worst), (None, None));
        assert_eq!(r.record, Record { wins: 1, losses: 0 });
        assert_eq!(r.span_s, 1200.0);
        assert_eq!(r.played_s, 1200.0);
        assert_eq!(r.baseline.games, 0);
        assert!(r.games[0].deltas.is_empty());
        assert_eq!((r.prev, r.next), (None, None));
    }

    #[test]
    fn span_covers_first_start_to_last_end() {
        let mut games = vec![
            g(1, 0, "A", true, 1.0, 1.0),
            g(2, 3000, "A", false, 1.0, 1.0),
        ];
        games[1].length = Some(600.0);
        let r = review(&games, 1).unwrap();
        assert_eq!(r.span_s, 3600.0);
        assert_eq!(r.played_s, 1800.0);
        assert_eq!(r.end, at(3600));
        assert_eq!(r.streaks.current, -1);
    }
}
