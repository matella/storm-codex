//! Lineups, post-game and draft scenes: pure helpers ported from `castState.ts`, `PostGameSlide.tsx`,
//! `LineupsScene.tsx`, `DraftScene.tsx` and `draftOverlay.ts`.

use crate::model::{
    CastSeries, DraftOrder, DraftState, DraftStep, GameSummary, LineupMode, PlayerSummary, Side,
};

// ── lineups ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    A,
    B,
}

/// Mode A (before the series) shows recent heroes, B (between games) the heroes played in the series.
pub fn effective_lineup_mode(s: &CastSeries) -> Mode {
    match s.lineup_mode {
        LineupMode::A => Mode::A,
        LineupMode::B => Mode::B,
        LineupMode::Auto if s.games.iter().any(|g| g.winner.is_some()) => Mode::B,
        LineupMode::Auto => Mode::A,
    }
}

pub fn player_name(name: &str, i: usize) -> String {
    if name.trim().is_empty() {
        format!("Player {}", i + 1)
    } else {
        name.to_string()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlayedHero {
    pub game: u32,
    pub hero: String,
}

/// Heroes played in the series, per player (team index 0-4), in game order. A summary row (same side) is
/// attached to a player only by (1) identical linked toon, then (2) identical name (trimmed, case-insensitive)
/// unless both have a toon and they differ, NEVER by position.
pub fn series_player_heroes(s: &CastSeries, side: Side) -> Vec<Vec<PlayedHero>> {
    let team = if side == Side::Blue { &s.blue } else { &s.red };
    let mut out: Vec<Vec<PlayedHero>> = team.players.iter().map(|_| vec![]).collect();
    for g in &s.games {
        let rows: Vec<&PlayerSummary> = g
            .summary
            .iter()
            .flat_map(|sm| sm.players.iter())
            .filter(|r| r.side == Some(side) && !r.hero.is_empty())
            .collect();
        let mut used = vec![false; rows.len()];
        let mut done = vec![false; team.players.len()];
        for (i, p) in team.players.iter().enumerate() {
            let Some(toon) = p.toon.as_deref().filter(|t| !t.is_empty()) else {
                continue;
            };
            if let Some(k) = rows
                .iter()
                .enumerate()
                .position(|(k, r)| !used[k] && r.toon == toon)
            {
                used[k] = true;
                done[i] = true;
                out[i].push(PlayedHero {
                    game: g.number,
                    hero: rows[k].hero.clone(),
                });
            }
        }
        for (i, p) in team.players.iter().enumerate() {
            let name = p.name.trim().to_lowercase();
            if done[i] || name.is_empty() {
                continue;
            }
            let ptoon = p.toon.as_deref().filter(|t| !t.is_empty());
            let found = rows.iter().enumerate().position(|(k, r)| {
                !used[k]
                    && !(ptoon.is_some_and(|t| !r.toon.is_empty() && r.toon != t))
                    && r.name.trim().to_lowercase() == name
            });
            if let Some(k) = found {
                used[k] = true;
                out[i].push(PlayedHero {
                    game: g.number,
                    hero: rows[k].hero.clone(),
                });
            }
        }
    }
    out
}

/// Query string of `GET /api/cast/lineup`: blank toon = unlinked player. `None` when nobody is linked.
pub fn lineup_query(s: &CastSeries) -> Option<String> {
    let toons = |t: &crate::model::Team| {
        t.players
            .iter()
            .map(|p| p.toon.clone().unwrap_or_default())
            .collect::<Vec<_>>()
    };
    let (b, r) = (toons(&s.blue), toons(&s.red));
    if b.iter().chain(r.iter()).all(String::is_empty) {
        return None;
    }
    let enc = |v: Vec<String>| v.iter().map(|t| encode(t)).collect::<Vec<_>>().join("%2C");
    Some(format!("blue={}&red={}", enc(b), enc(r)))
}

/// `encodeURIComponent` for the characters toons contain.
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Two first alphanumeric characters of a team or hero name, "?" when none (`portrait.tsx`).
pub fn team_initials(name: &str) -> String {
    let s: String = name
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(2)
        .collect::<String>()
        .to_uppercase();
    if s.is_empty() {
        "?".into()
    } else {
        s
    }
}

// ── post-game ──────────────────────────────────────────────────────────────────

pub const TAB_KEYS: [&str; 4] = ["hero_damage", "healing", "siege_damage", "takedowns"];
pub const TAB_LABELS: [&str; 4] = ["HERO DAMAGE", "HEALING", "SIEGE", "TAKEDOWNS"];

/// Value of the breakdown tab `tab` (0-3) for a player.
pub fn tab_value(p: &PlayerSummary, tab: usize) -> f64 {
    match tab {
        0 => p.hero_damage,
        1 => p.healing,
        2 => p.siege_damage,
        _ => p.takedowns as f64,
    }
}

/// Per-player values (bars): one decimal from 1000 up (161.6k).
pub fn fmt_bar(n: f64) -> String {
    if n >= 1000.0 {
        format!("{:.1}k", n / 1000.0)
    } else {
        format!("{}", js_round(n))
    }
}

/// Team totals: 214k / 82.4k / 950.
pub fn fmt_stat(n: f64) -> String {
    if n >= 100_000.0 {
        format!("{}k", js_round(n / 1000.0))
    } else {
        fmt_bar(n)
    }
}

/// `Math.round`: halves go up.
fn js_round(n: f64) -> i64 {
    (n + 0.5).floor() as i64
}

/// `m:ss`, signed (`fmtClock`).
pub fn fmt_clock(sec: f64) -> String {
    let s = sec.abs().floor() as u64;
    format!(
        "{}{}:{:02}",
        if sec < 0.0 { "−" } else { "" },
        s / 60,
        s % 60
    )
}

pub fn side_sum(sm: &GameSummary, side: Side, f: impl Fn(&PlayerSummary) -> f64) -> f64 {
    sm.players
        .iter()
        .filter(|p| p.side == Some(side))
        .map(f)
        .sum()
}

/// Players of one side sorted by the tab value, best first (stable).
pub fn bars_side(sm: &GameSummary, side: Side, tab: usize) -> Vec<&PlayerSummary> {
    let mut v: Vec<&PlayerSummary> = sm.players.iter().filter(|p| p.side == Some(side)).collect();
    v.sort_by(|a, b| {
        tab_value(b, tab)
            .partial_cmp(&tab_value(a, tab))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    v
}

/// Width of a bar in percent, relative to the best player of the game.
pub fn bar_width(sm: &GameSummary, p: &PlayerSummary, tab: usize) -> i64 {
    let max = sm
        .players
        .iter()
        .map(|p| tab_value(p, tab))
        .fold(1.0_f64, f64::max);
    js_round(tab_value(p, tab) * 100.0 / max)
}

/// XP-lead curve: `(line path, area path)` in a 400x110 viewbox, zero line at y = 55.
pub fn xp_paths(lead: &[(f64, f64)], length: f64) -> (String, String) {
    let max_t = lead.iter().map(|(t, _)| *t).fold(length.max(1.0), f64::max);
    let max_v = lead.iter().map(|(_, v)| v.abs()).fold(1.0_f64, f64::max);
    let pts: Vec<String> = lead
        .iter()
        .map(|(t, v)| format!("{:.1},{:.1}", t / max_t * 400.0, 55.0 - v / max_v * 50.0))
        .collect();
    let line = format!("M0,55 L{}", pts.join(" L"));
    let last_t = lead.last().map(|(t, _)| *t).unwrap_or(0.0);
    let area = format!("{line} L{:.1},55 Z", last_t / max_t * 400.0);
    (line, area)
}

// ── draft ──────────────────────────────────────────────────────────────────────

pub const PHASE_SECONDS: f64 = 45.0;

pub fn side_of_step(d: &DraftState, step: &DraftStep) -> Side {
    match (step.order, d.first_pick) {
        (DraftOrder::First, s) => s,
        (DraftOrder::Second, Side::Blue) => Side::Red,
        (DraftOrder::Second, Side::Red) => Side::Blue,
    }
}

/// Start index of the block of consecutive same-order steps holding the cursor (`None` = draft over).
pub fn phase_start(d: &DraftState) -> Option<usize> {
    let order = d.steps.get(d.cursor)?.order;
    let mut s = d.cursor;
    while s > 0 && d.steps[s - 1].order == order {
        s -= 1;
    }
    Some(s)
}

/// Replay prepared but nothing revealed yet.
pub fn is_replay_ready(d: &DraftState) -> bool {
    d.replay.as_ref().is_some_and(|r| r.current.is_some()) && d.cursor == 0
}

pub fn in_replay(d: &DraftState) -> bool {
    d.replay.as_ref().is_some_and(|r| r.current.is_some())
}

/// Fraction (0-1) of the timer bar: local 45 s countdown outside replay; in replay driven by the next
/// auto-play tick, empty when manual or paused.
pub fn timer_fraction(d: &DraftState, now_ms: f64, local_secs: f64, fallback_secs: f64) -> f64 {
    match d.replay.as_ref().filter(|r| r.current.is_some()) {
        None => local_secs / fallback_secs,
        Some(r) => match r.next_at_ms {
            Some(at) if r.playing && r.interval_ms > 0.0 => {
                ((at - now_ms) / r.interval_ms).clamp(0.0, 1.0)
            }
            _ => 0.0,
        },
    }
}

pub const SKINS: [&str; 4] = ["nexus", "glass", "tactical", "mono"];

/// Skin asked for by `?skin=` (fallback `nexus`).
pub fn skin(search: &str) -> &'static str {
    let v = search
        .trim_start_matches('?')
        .split('&')
        .find_map(|kv| kv.strip_prefix("skin="))
        .unwrap_or("nexus");
    SKINS.iter().find(|s| **s == v).copied().unwrap_or("nexus")
}

/// One pick slot of the timeline: `Some((side, ordinal within the side))`.
pub struct PickSlot {
    pub index: usize,
    pub side: Side,
    pub ordinal: usize,
}

pub fn pick_slots(d: &DraftState) -> Vec<PickSlot> {
    let (mut bc, mut rc) = (0, 0);
    d.steps
        .iter()
        .enumerate()
        .filter(|(_, s)| s.action == crate::model::DraftAction::Pick)
        .map(|(index, s)| {
            let side = side_of_step(d, s);
            let c = if side == Side::Blue { &mut bc } else { &mut rc };
            let ordinal = *c;
            *c += 1;
            PickSlot {
                index,
                side,
                ordinal,
            }
        })
        .collect()
}

/// Ban slots in order: `(step index, side, separator before)` — a separator sits between two ban phases.
pub fn ban_slots(d: &DraftState) -> Vec<(usize, Side, bool)> {
    let bans: Vec<(usize, &DraftStep)> = d
        .steps
        .iter()
        .enumerate()
        .filter(|(_, s)| s.action == crate::model::DraftAction::Ban)
        .collect();
    bans.iter()
        .enumerate()
        .map(|(k, (i, s))| (*i, side_of_step(d, s), k > 0 && i - bans[k - 1].0 > 1))
        .collect()
}

/// Draft portrait fallback (`initials` of api.ts): the two first characters, upper case.
pub fn draft_initials(name: &str) -> String {
    if name.is_empty() {
        "··".into()
    } else {
        name.chars().take(2).collect::<String>().to_uppercase()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CastPlayer, DraftAction, DraftReplay, DraftTeam, Game, Team};

    fn player(name: &str, toon: Option<&str>) -> CastPlayer {
        CastPlayer {
            name: name.into(),
            tagline: String::new(),
            toon: toon.map(str::to_string),
        }
    }
    fn row(side: Side, name: &str, toon: &str, hero: &str) -> PlayerSummary {
        PlayerSummary {
            side: Some(side),
            name: name.into(),
            toon: toon.into(),
            hero: hero.into(),
            ..PlayerSummary::default()
        }
    }
    fn series() -> CastSeries {
        serde_json::from_str(
            r#"{"active":true,"blue":{"name":"A"},"red":{"name":"B"},"bo":3,"format":"ban","score_override":null,
                "pool":[],"games":[],"scene":"lineups","slide":{"game":null,"index":0,"auto":false,"interval_ms":1000,"since_ms":0}}"#,
        )
        .unwrap()
    }

    #[test]
    fn lineup_mode_follows_the_results_unless_forced() {
        let mut s = series();
        assert_eq!(effective_lineup_mode(&s), Mode::A);
        s.games.push(Game {
            number: 1,
            map: "X".into(),
            winner: Some(Side::Red),
            takedowns: (0, 0),
            summary: None,
        });
        assert_eq!(effective_lineup_mode(&s), Mode::B);
        s.lineup_mode = LineupMode::A;
        assert_eq!(effective_lineup_mode(&s), Mode::A);
    }

    #[test]
    fn heroes_attach_by_toon_then_name_never_by_position() {
        let mut s = series();
        s.blue = Team {
            players: vec![
                player("Ana", Some("T1")),
                player("bob ", None),
                player("Cy", Some("T3")),
            ],
            ..Team::default()
        };
        let mut sm = crate::logic::tests::summary();
        sm.players = vec![
            row(Side::Blue, "zzz", "T1", "Zeratul"), // toon match for Ana, name differs
            row(Side::Blue, "BOB", "", "Abathur"),   // name match for bob
            row(Side::Blue, "Cy", "OTHER", "Murky"), // same name, different toon: not Cy
            row(Side::Red, "Ana", "T1", "Nova"),     // other side
        ];
        s.games.push(Game {
            number: 1,
            map: "X".into(),
            winner: None,
            takedowns: (0, 0),
            summary: Some(sm),
        });
        let h = series_player_heroes(&s, Side::Blue);
        assert_eq!(
            h[0],
            vec![PlayedHero {
                game: 1,
                hero: "Zeratul".into()
            }]
        );
        assert_eq!(
            h[1],
            vec![PlayedHero {
                game: 1,
                hero: "Abathur".into()
            }]
        );
        assert!(h[2].is_empty());
    }

    #[test]
    fn lineup_query_skips_unlinked_series() {
        let mut s = series();
        assert_eq!(lineup_query(&s), None);
        s.blue.players = vec![player("a", Some("2-Hero-1-1")), player("b", None)];
        assert_eq!(lineup_query(&s).unwrap(), "blue=2-Hero-1-1%2C&red=");
    }

    #[test]
    fn numbers_are_formatted_like_the_react_slide() {
        assert_eq!(fmt_bar(950.0), "950");
        assert_eq!(fmt_bar(161_600.0), "161.6k");
        assert_eq!(fmt_stat(214_000.0), "214k");
        assert_eq!(fmt_stat(82_400.0), "82.4k");
        assert_eq!(fmt_stat(99_999.0), "100.0k");
        assert_eq!(fmt_clock(754.9), "12:34");
        assert_eq!(fmt_clock(-5.0), "−0:05");
        assert_eq!(team_initials("!a-b c"), "AB");
        assert_eq!(team_initials("!!"), "?");
    }

    #[test]
    fn bars_are_sorted_and_scaled_on_the_best_player() {
        let mut sm = crate::logic::tests::summary();
        sm.players = vec![
            PlayerSummary {
                side: Some(Side::Blue),
                hero_damage: 100.0,
                ..Default::default()
            },
            PlayerSummary {
                side: Some(Side::Blue),
                hero_damage: 400.0,
                ..Default::default()
            },
            PlayerSummary {
                side: Some(Side::Red),
                hero_damage: 200.0,
                ..Default::default()
            },
        ];
        let blue = bars_side(&sm, Side::Blue, 0);
        assert_eq!(blue[0].hero_damage, 400.0);
        assert_eq!(bar_width(&sm, blue[0], 0), 100);
        assert_eq!(bar_width(&sm, blue[1], 0), 25);
        assert_eq!(bar_width(&sm, &sm.players[2], 0), 50);
    }

    #[test]
    fn xp_curve_is_anchored_on_the_zero_line() {
        let (line, area) = xp_paths(&[(0.0, 0.0), (300.0, 10.0), (600.0, -10.0)], 600.0);
        assert_eq!(line, "M0,55 L0.0,55.0 L200.0,5.0 L400.0,105.0");
        assert_eq!(area, format!("{line} L400.0,55 Z"));
    }

    fn draft() -> DraftState {
        use DraftOrder::{First, Second};
        let step = |order, action| DraftStep { order, action };
        DraftState {
            format: "standard".into(),
            first_pick: Side::Red,
            map: "Sky Temple".into(),
            blue: DraftTeam::default(),
            red: DraftTeam::default(),
            score: (0, 0),
            bo: 1,
            steps: vec![
                step(First, DraftAction::Ban),
                step(Second, DraftAction::Ban),
                step(First, DraftAction::Pick),
                step(Second, DraftAction::Pick),
                step(Second, DraftAction::Pick),
                step(First, DraftAction::Ban),
            ],
            cursor: 4,
            assignments: vec![None; 6],
            manual_unavailable: vec![],
            series_bans: vec![],
            replay: None,
        }
    }

    #[test]
    fn draft_sides_phases_and_slots() {
        let d = draft();
        assert_eq!(side_of_step(&d, &d.steps[0]), Side::Red);
        assert_eq!(side_of_step(&d, &d.steps[1]), Side::Blue);
        assert_eq!(phase_start(&d), Some(3));
        let picks = pick_slots(&d);
        assert_eq!(
            picks
                .iter()
                .map(|p| (p.index, p.side, p.ordinal))
                .collect::<Vec<_>>(),
            [(2, Side::Red, 0), (3, Side::Blue, 0), (4, Side::Blue, 1)]
        );
        assert_eq!(
            ban_slots(&d).iter().map(|b| b.2).collect::<Vec<_>>(),
            [false, false, true]
        );
        let mut over = d;
        over.cursor = 6;
        assert_eq!(phase_start(&over), None);
    }

    #[test]
    fn timer_is_local_outside_replay_and_driven_by_the_next_tick_in_replay() {
        let mut d = draft();
        assert_eq!(timer_fraction(&d, 0.0, 22.5, 45.0), 0.5);
        d.replay = Some(DraftReplay {
            current: Some(0),
            playing: true,
            next_at_ms: Some(1500.0),
            interval_ms: 1000.0,
        });
        assert_eq!(timer_fraction(&d, 1000.0, 45.0, 45.0), 0.5);
        assert_eq!(timer_fraction(&d, 9999.0, 45.0, 45.0), 0.0);
        d.replay.as_mut().unwrap().playing = false;
        assert_eq!(timer_fraction(&d, 1000.0, 45.0, 45.0), 0.0);
        d.cursor = 0;
        assert!(is_replay_ready(&d));
    }

    #[test]
    fn skin_falls_back_to_nexus() {
        assert_eq!(skin("?skin=glass"), "glass");
        assert_eq!(skin("?skin=nope"), "nexus");
        assert_eq!(skin(""), "nexus");
    }
}
