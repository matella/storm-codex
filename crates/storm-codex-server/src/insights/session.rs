//! Session review (spec `docs/specs/2026-11-19-session-review.md`): the operator's games split into play sessions
//! (a gap of more than 3 h between two game starts), and one session against the operator's own baseline (the 90
//! days before it, in the game modes played in the session). Pure logic over the insights corpus; nothing is written.

use super::cache::{Corpus, Game, PlayerRow};
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Debug, Serialize, PartialEq, Default)]
pub struct SessionMeta {
    pub manual: bool,
    pub open: bool,
    pub name: Option<String>,
    pub opened_at: Option<DateTime<Utc>>,
    pub closed_at: Option<DateTime<Utc>>,
}

#[derive(Debug)]
pub struct Window {
    pub id: i64, // negative wire ID
    pub start: DateTime<Utc>,
    pub end: Option<DateTime<Utc>>,
    pub name: Option<String>,
}

impl Window {
    pub fn meta(&self) -> SessionMeta {
        SessionMeta {
            manual: true,
            open: self.end.is_none(),
            name: self.name.clone(),
            opened_at: Some(self.start),
            closed_at: self.end,
        }
    }
}

pub struct Group<'a> {
    id: i64,
    start: DateTime<Utc>,
    window: Option<&'a Window>,
    games: Vec<&'a MyGame>,
}

impl Group<'_> {
    fn meta(&self) -> SessionMeta {
        self.window.map(Window::meta).unwrap_or_default()
    }

    fn end(&self) -> DateTime<Utc> {
        end_of(&self.games).unwrap_or_else(|| self.window.and_then(|w| w.end).unwrap_or(self.start))
    }
}

/// Windows are sorted by start (then creation ID); `Some(None)` forces automatic grouping.
/// The original dated game list is retained separately for the review baseline.
pub fn groups<'a>(
    games: &'a [MyGame],
    windows: &'a [Window],
    overrides: &HashMap<i64, Option<i64>>,
    filtered: bool,
) -> Vec<Group<'a>> {
    let mut manual: Vec<_> = windows
        .iter()
        .map(|w| Group {
            id: w.id,
            start: w.start,
            window: Some(w),
            games: Vec::new(),
        })
        .collect();
    let by_id: HashMap<_, _> = windows.iter().enumerate().map(|(i, w)| (w.id, i)).collect();
    let mut automatic: Vec<Group<'_>> = Vec::new();
    for g in games {
        let target = match overrides.get(&g.id) {
            Some(id) => id.and_then(|id| by_id.get(&id).copied()),
            None => windows
                .partition_point(|w| w.start <= g.played_at)
                .checked_sub(1)
                .filter(|&i| windows[i].end.is_none_or(|end| g.played_at < end)),
        };
        if let Some(i) = target {
            manual[i].games.push(g);
        } else {
            let group = automatic.last_mut().filter(|group| {
                group
                    .games
                    .last()
                    .is_some_and(|last| g.played_at - last.played_at <= Duration::seconds(GAP_S))
            });
            if let Some(group) = group {
                group.games.push(g);
            } else {
                automatic.push(Group {
                    id: g.id,
                    start: g.played_at,
                    window: None,
                    games: vec![g],
                });
            }
        }
    }
    automatic.extend(
        manual
            .into_iter()
            .filter(|g| !filtered || !g.games.is_empty()),
    );
    automatic.sort_unstable_by_key(|g| (g.start, g.id));
    automatic
}

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

/// End of a session: the last start plus the last game's length.
fn end_of(games: &[&MyGame]) -> Option<DateTime<Utc>> {
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
    #[serde(flatten)]
    pub meta: SessionMeta,
}

/// The `limit` most recent sessions, newest first.
pub fn list(sessions: &[Group<'_>], limit: usize, prefer_open: bool) -> Vec<SessionItem> {
    let open = prefer_open
        .then(|| {
            sessions
                .iter()
                .find(|g| g.window.is_some_and(|w| w.end.is_none()))
        })
        .flatten();
    open.into_iter()
        .chain(
            sessions
                .iter()
                .rev()
                .filter(|g| open.is_none_or(|w| w.id != g.id)),
        )
        .take(limit)
        .map(|g| SessionItem {
            id: g.id,
            start: g.games.first().map_or(g.start, |m| m.played_at),
            end: g.end(),
            games: g.games.len(),
            wins: g.games.iter().filter(|m| m.win).count(),
            meta: g.meta(),
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

/// Number of displayed deltas better than the baseline; no score without a comparison.
fn score(d: &BTreeMap<&'static str, f64>) -> Option<u32> {
    (!d.is_empty()).then(|| {
        d.iter()
            .filter(|&(&stat, &v)| if stat == "deaths" { v < 0.0 } else { v > 0.0 })
            .count() as u32
    })
}

/// Best and worst game (indices into `games`); at least two games must have a score.
fn best_worst(games: &[&MyGame], scores: &[Option<u32>]) -> Option<(usize, usize)> {
    let ranked = scores
        .iter()
        .enumerate()
        .filter_map(|(i, s)| Some((i, (*s)?)));
    let cmp = |&(a, sa): &(usize, u32), &(b, sb): &(usize, u32)| {
        sa.cmp(&sb)
            .then_with(|| games[a].kda().total_cmp(&games[b].kda()))
    };
    let (best, _) = ranked.clone().max_by(cmp)?;
    let (worst, _) = ranked.min_by(cmp)?;
    (best != worst).then_some((best, worst))
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
    #[serde(flatten)]
    pub meta: SessionMeta,
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
    session: &[&MyGame],
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

/// Review of an automatic anchor or a stable manual ID.
pub fn review(games: &[MyGame], sessions: &[Group<'_>], id: i64) -> Option<Review> {
    let k = sessions.iter().position(|g| g.id == id)?;
    let group = &sessions[k];
    let session = &group.games;
    let start = session.first().map_or(group.start, |g| g.played_at);
    let end = group.end();
    let cutoff = group.start;
    let from = cutoff - Duration::days(BASELINE_DAYS);
    let members: HashSet<_> = session.iter().map(|g| g.id).collect();
    let modes: Vec<Option<i32>> = {
        let set: HashSet<Option<i32>> = session.iter().map(|g| g.mode).collect();
        let mut v: Vec<_> = set.into_iter().collect();
        v.sort_unstable();
        v
    };
    let base: Vec<&MyGame> = games
        .iter()
        .filter(|g| {
            g.played_at >= from
                && g.played_at < cutoff
                && modes.contains(&g.mode)
                && !members.contains(&g.id)
        })
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
        let d = by_stat(&deltas(g, &avg));
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
            deltas: d,
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
        span_s: if session.is_empty() {
            0.0
        } else {
            (end - start).num_milliseconds() as f64 / 1000.0
        },
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
        prev: k.checked_sub(1).map(|p| sessions[p].id),
        next: sessions.get(k + 1).map(|g| g.id),
        meta: group.meta(),
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

    fn review(games: &[MyGame], id: i64) -> Option<Review> {
        super::review(games, &groups(games, &[], &HashMap::new(), false), id)
    }

    #[test]
    fn splits_on_gaps_over_three_hours_only() {
        let games = [
            g(1, 0, "A", true, 1.0, 1.0),
            g(2, GAP_S, "A", true, 1.0, 1.0), // exactly 3 h: same session
            g(3, 2 * GAP_S + 1, "A", true, 1.0, 1.0), // 3 h + 1 s: new one
        ];
        assert!(groups(&[], &[], &HashMap::new(), false).is_empty());
        let groups = groups(&games, &[], &HashMap::new(), false);
        let l = list(&groups, 20, false);
        assert_eq!(l[0].games, 1);
        assert_eq!(list(&groups, 1, false).len(), 1);
        assert_eq!(
            l.iter().map(|s| s.id).collect::<Vec<_>>(),
            vec![3, 1],
            "newest first"
        );
        assert_eq!(l[1].games, 2);
        assert_eq!(l[1].end, at(GAP_S + 1200));
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
        let score_game = |game: &MyGame| score(&by_stat(&deltas(game, &avg)));
        assert_eq!(score_game(&top), Some(7));
        // equal is neither above nor below; more deaths is worse
        let even = g(2, 10, "A", true, 5.0, 9.0);
        assert_eq!(score_game(&even), Some(0));
        let games = [top, even.clone(), g(3, 20, "A", true, 5.0, 1.0)];
        // game 3: only deaths better → 1; game 2: 0
        let scores: Vec<_> = games.iter().map(score_game).collect();
        assert_eq!(scores, vec![Some(7), Some(0), Some(1)]);
        assert_eq!(best_worst(&games.each_ref(), &scores), Some((0, 1)));
        // same score: the KDA ratio decides (a loss can be the best game)
        let tie = [g(1, 0, "A", true, 2.0, 2.0), g(2, 1, "A", false, 8.0, 2.0)];
        assert_eq!(
            best_worst(&tie.each_ref(), &[Some(3), Some(3)]),
            Some((1, 0))
        );
        // all equal: still two different games
        let same = [even.clone(), even];
        let (b, w) = best_worst(&same.each_ref(), &[Some(0), Some(0)]).unwrap();
        assert_ne!(b, w);
        assert_eq!(best_worst(&games.each_ref()[..1], &[Some(0)]), None);
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
    fn multi_game_session_without_baseline_has_no_picks() {
        let games = [g(1, 0, "A", true, 6.0, 1.0), g(2, 60, "B", false, 2.0, 4.0)];
        let r = review(&games, 1).unwrap();
        assert_eq!(r.games.len(), 2);
        assert!(r.games.iter().all(|g| g.deltas.is_empty()));
        assert_eq!((r.best, r.worst), (None, None));
    }

    #[test]
    fn only_games_with_deltas_can_rank() {
        let mut games = vec![
            g(1, 0, "A", true, 5.0, 2.0),
            g(2, 14_000, "A", true, 6.0, 1.0),
            g(3, 14_060, "B", false, 2.0, 4.0),
        ];
        games[2].stats = [None; 7];
        let r = review(&games, 2).unwrap();
        assert_eq!((r.best, r.worst), (None, None));

        games[2].stats[0] = Some(5.0); // a zero delta is still comparable
        games.push(g(4, 14_120, "C", false, 0.0, 0.0));
        games[3].stats = [None; 7];
        let r = review(&games, 2).unwrap();
        assert_eq!((r.best, r.worst), (Some(2), Some(3)));
    }

    #[test]
    fn ranking_does_not_count_deltas_displayed_as_zero() {
        let games = [
            g(1, 0, "A", true, 5.0, 2.0),
            g(2, 14_000, "A", true, 5.01, 2.01),
            g(3, 14_060, "A", false, 5.0, 1.96),
        ];
        let r = review(&games, 2).unwrap();
        assert!(r.games.iter().all(|g| g.deltas.values().all(|&d| d == 0.0)));
        assert_eq!(
            (r.best, r.worst),
            (Some(3), Some(2)),
            "displayed scores tie; higher KDA wins"
        );
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

    fn window(id: i64, start: i64, end: Option<i64>) -> Window {
        Window {
            id,
            start: at(start),
            end: end.map(at),
            name: None,
        }
    }

    fn ids(group: &Group<'_>) -> Vec<i64> {
        group.games.iter().map(|g| g.id).collect()
    }

    #[test]
    fn window_is_start_inclusive_end_exclusive_and_each_game_has_one_home() {
        let games = [
            g(1, 99, "A", true, 1.0, 1.0),
            g(2, 100, "A", true, 1.0, 1.0),
            g(3, 200, "A", true, 1.0, 1.0),
        ];
        let windows = [window(-1, 100, Some(200))];
        let groups = groups(&games, &windows, &HashMap::new(), false);
        let manual = groups.iter().find(|g| g.id == -1).unwrap();
        assert_eq!(ids(manual), [2]);
        let total: usize = groups.iter().map(|g| g.games.len()).sum();
        assert_eq!(total, games.len(), "no game is lost or duplicated");
    }

    #[test]
    fn an_open_window_is_unbounded_and_a_late_replay_joins_it() {
        let games = [
            g(1, 100, "A", true, 1.0, 1.0),
            g(2, 10 * DAY, "A", true, 1.0, 1.0),
        ];
        let windows = [window(-1, 100, None)];
        let groups = groups(&games, &windows, &HashMap::new(), false);
        assert_eq!(ids(groups.iter().find(|g| g.id == -1).unwrap()), [1, 2]);
    }

    #[test]
    fn overrides_win_and_none_forces_automatic_grouping() {
        let games = [
            g(1, 50, "A", true, 1.0, 1.0),
            g(2, 150, "A", true, 1.0, 1.0),
            g(3, 250, "A", true, 1.0, 1.0),
        ];
        let windows = [window(-1, 100, Some(200)), window(-2, 300, Some(400))];
        let overrides = HashMap::from([(1, Some(-2)), (2, None), (3, Some(-1))]);
        let groups = groups(&games, &windows, &overrides, false);
        let find = |id| ids(groups.iter().find(|g| g.id == id).unwrap());
        assert_eq!(find(-1), [3]);
        assert_eq!(find(-2), [1]);
        assert_eq!(find(2), [2], "forced automatic: its own automatic session");
    }

    #[test]
    fn unassigned_games_keep_the_three_hour_rule_around_manual_games() {
        let games = [
            g(1, 0, "A", true, 1.0, 1.0),
            g(2, GAP_S / 2, "A", true, 1.0, 1.0), // taken by the manual window
            g(3, GAP_S + 1, "A", true, 1.0, 1.0), // > 3 h after game 1, but game 2 is not automatic
        ];
        let windows = [window(-1, GAP_S / 2, Some(GAP_S / 2 + 1))];
        let groups = groups(&games, &windows, &HashMap::new(), false);
        let automatic: Vec<_> = groups.iter().filter(|g| g.window.is_none()).collect();
        assert_eq!(automatic.len(), 2);
    }

    #[test]
    fn empty_manual_session_has_no_games_numbers_or_picks_and_filters_hide_it() {
        let games = [g(1, 0, "A", true, 1.0, 1.0)];
        let windows = [window(-1, 5 * DAY, None)];
        let all = groups(&games, &windows, &HashMap::new(), false);
        let item = &list(&all, 20, true)[0];
        assert_eq!((item.id, item.games, item.wins), (-1, 0, 0));
        assert_eq!((item.start, item.end), (at(5 * DAY), at(5 * DAY)));
        assert!(item.meta.open && item.meta.manual);
        let r = super::review(&games, &all, -1).unwrap();
        assert!(r.games.is_empty());
        assert_eq!((r.best, r.worst), (None, None));
        let filtered = groups(&games, &windows, &HashMap::new(), true);
        assert!(super::review(&games, &filtered, -1).is_none());
    }

    #[test]
    fn prefer_open_puts_the_open_window_first_without_changing_navigation() {
        let games = [
            g(1, 0, "A", true, 1.0, 1.0),
            g(2, 10 * DAY, "A", true, 1.0, 1.0),
        ];
        let windows = [window(-1, 5 * DAY, None)];
        let overrides = HashMap::from([(2, None)]);
        let groups = groups(&games, &windows, &overrides, false);
        assert_eq!(list(&groups, 1, false)[0].id, 2);
        assert_eq!(list(&groups, 1, true)[0].id, -1);
        let r = super::review(&games, &groups, -1).unwrap();
        assert_eq!((r.prev, r.next), (Some(1), Some(2)));
    }

    #[test]
    fn a_game_moved_in_from_before_the_window_is_not_in_its_own_baseline() {
        let games = [
            g(1, 0, "A", true, 10.0, 1.0),
            g(2, DAY, "A", true, 2.0, 1.0),
            g(3, 3 * DAY, "A", true, 6.0, 1.0),
        ];
        let windows = [window(-1, 3 * DAY, Some(4 * DAY))];
        let overrides = HashMap::from([(1, Some(-1))]);
        let groups = groups(&games, &windows, &overrides, false);
        let r = super::review(&games, &groups, -1).unwrap();
        assert_eq!(
            r.games.iter().map(|g| g.match_id).collect::<Vec<_>>(),
            [1, 3]
        );
        assert_eq!(r.baseline.games, 1);
        assert_eq!(r.baseline.averages["kills"], 2.0);
    }
}
