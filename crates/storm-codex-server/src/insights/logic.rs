//! Pure logic of the local insights (spec `docs/specs/2026-11-17-local-insights-design.md`): team fights,
//! time windows, camp reasons, early leads, level leads. Input = the storm-stats `match` JSON and the players'
//! `(toon, team)`; no I/O, so everything here is unit-tested without a database.
//!
//! Times are game seconds as written by storm-stats (`time` fields). Team indices are 0 (blue) and 1 (red).

use serde::Serialize;
use serde_json::Value as J;
use std::collections::HashMap;

/// A death joins the current fight when it comes at most this long after the fight's previous death…
pub const FIGHT_GAP_S: f64 = 15.0;
/// …and at most this far (map units) from the fight's centre.
pub const FIGHT_RADIUS: f64 = 30.0;
/// What happens up to this long after a fight is reported as its outcome.
pub const AFTER_FIGHT_S: f64 = 30.0;
/// Objective events of one team this close together are one event (Sky Temple shots, Garden plants…).
const OBJECTIVE_MERGE_S: f64 = 20.0;
/// Storm-stats takedown positions are fixed point; structures and map events use map units.
const FIXED_POINT: f64 = 4096.0;

/// One hero death, flattened from `match.takedowns[]`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Death {
    pub time: f64,
    pub x: f64,
    pub y: f64,
    /// victim's team (0/1)
    pub team: u8,
    pub victim: String,
    pub victim_hero: String,
    /// toons of the killers (takedown participants)
    pub killers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TimedTeam {
    pub time: f64,
    pub team: u8,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Fight {
    pub start: f64,
    pub end: f64,
    /// heroes lost per team
    pub lost: [u32; 2],
    /// team that lost fewer heroes; `None` = even
    pub winner: Option<u8>,
    /// victims, in order
    pub deaths: Vec<Death>,
    /// within `AFTER_FIGHT_S` after the fight: structures destroyed BY each team
    pub structures_after: [u32; 2],
    /// camps taken by each team
    pub camps_after: [u32; 2],
    /// objective events won by each team
    pub objectives_after: [u32; 2],
}

/// The match facts the insights need, read once from the storm-stats JSON.
#[derive(Debug, Clone, Default)]
pub struct MatchFacts {
    pub deaths: Vec<Death>,
    /// destroyed structures: `team` = the team that DESTROYED it
    pub structures: Vec<TimedTeam>,
    pub camps: Vec<TimedTeam>,
    pub objectives: Vec<TimedTeam>,
    /// `(level, time)` the team reached it, by level
    pub level_times: [Vec<(u32, f64)>; 2],
    /// cumulative team XP samples `(time, xp)`, every 60 s
    pub xp: [Vec<(f64, f64)>; 2],
    /// level lead of team 0 over time: `(start, end, diff)`
    pub level_adv: Vec<(f64, f64, f64)>,
}

fn num(v: &J) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn team_of(v: &J) -> Option<u8> {
    let t = num(v)?;
    if t == 0.0 {
        Some(0)
    } else if t == 1.0 {
        Some(1)
    } else {
        None
    }
}

impl MatchFacts {
    /// `teams` maps a player's toon to their team. Missing or odd fields are skipped, never an error:
    /// a game read by an older parser simply has fewer facts.
    pub fn from_match(m: &J, teams: &HashMap<String, u8>) -> Self {
        let mut f = MatchFacts::default();
        for t in m["takedowns"].as_array().into_iter().flatten() {
            let victim = t["victim"]["player"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            let (Some(time), Some(&team)) = (num(&t["time"]), teams.get(&victim)) else {
                continue;
            };
            f.deaths.push(Death {
                time,
                x: num(&t["x"]).unwrap_or(0.0) / FIXED_POINT,
                y: num(&t["y"]).unwrap_or(0.0) / FIXED_POINT,
                team,
                victim_hero: t["victim"]["hero"].as_str().unwrap_or_default().to_string(),
                victim,
                killers: t["killers"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|k| k["player"].as_str().map(str::to_string))
                    .collect(),
            });
        }
        f.deaths.sort_by(|a, b| a.time.total_cmp(&b.time));

        if let Some(st) = m["structures"].as_object() {
            for s in st.values() {
                if let (Some(time), Some(owner)) = (num(&s["destroyed"]), team_of(&s["team"])) {
                    f.structures.push(TimedTeam {
                        time,
                        team: 1 - owner,
                        label: s["name"].as_str().unwrap_or("Structure").to_string(),
                    });
                }
            }
        }
        f.structures.sort_by(|a, b| a.time.total_cmp(&b.time));

        for c in m["mercs"]["captures"].as_array().into_iter().flatten() {
            if let (Some(time), Some(team)) = (num(&c["time"]), team_of(&c["team"])) {
                f.camps.push(TimedTeam {
                    time,
                    team,
                    label: c["type"].as_str().unwrap_or("Camp").to_string(),
                });
            }
        }
        f.camps.sort_by(|a, b| a.time.total_cmp(&b.time));

        f.objectives = objective_events(&m["objective"]);

        for team in 0..2u8 {
            let mut lv: Vec<(u32, f64)> = m["levelTimes"][team.to_string()]
                .as_object()
                .into_iter()
                .flatten()
                .filter_map(|(k, v)| Some((k.parse().ok()?, num(&v["time"])?)))
                .collect();
            lv.sort_by_key(|(l, _)| *l);
            f.level_times[team as usize] = lv;
        }

        for x in m["XPBreakdown"].as_array().into_iter().flatten() {
            let (Some(time), Some(team)) = (num(&x["time"]), team_of(&x["team"])) else {
                continue;
            };
            let total: f64 = ["MinionXP", "CreepXP", "StructureXP", "HeroXP", "TrickleXP"]
                .iter()
                .filter_map(|k| num(&x["breakdown"][*k]))
                .sum();
            f.xp[team as usize].push((time, total));
        }
        for s in &mut f.xp {
            s.sort_by(|a, b| a.0.total_cmp(&b.0));
        }

        for e in m["levelAdvTimeline"].as_array().into_iter().flatten() {
            if let (Some(a), Some(b), Some(d)) =
                (num(&e["start"]), num(&e["end"]), num(&e["levelDiff"]))
            {
                f.level_adv.push((a, b, d));
            }
        }
        f
    }
}

/// Map objective events of either team, from the per-map `match.objective` shapes: any object with a numeric
/// `time` (or `startTime`) and a `team` or `winner` of 0/1. Control samples, wave scores and tributes are not
/// events. Same-team events closer than `OBJECTIVE_MERGE_S` merge into the first one.
pub fn objective_events(obj: &J) -> Vec<TimedTeam> {
    fn walk(v: &J, out: &mut Vec<TimedTeam>) {
        match v {
            J::Array(a) => a.iter().for_each(|x| walk(x, out)),
            J::Object(o) => {
                let time = o.get("time").or_else(|| o.get("startTime")).and_then(num);
                let team = o
                    .get("team")
                    .and_then(team_of)
                    .or_else(|| o.get("winner").and_then(team_of));
                if let (Some(time), Some(team)) = (time, team) {
                    out.push(TimedTeam {
                        time,
                        team,
                        label: "Objective".into(),
                    });
                    return;
                }
                for (k, x) in o {
                    if !matches!(k.as_str(), "control" | "scores" | "waves" | "tributes") {
                        walk(x, out);
                    }
                }
            }
            _ => {}
        }
    }
    let mut raw = Vec::new();
    walk(obj, &mut raw);
    raw.sort_by(|a, b| a.time.total_cmp(&b.time));
    let mut out: Vec<TimedTeam> = Vec::new();
    let mut last: [Option<f64>; 2] = [None, None];
    for e in raw {
        let l = &mut last[e.team as usize];
        if l.is_some_and(|t| e.time - t <= OBJECTIVE_MERGE_S) {
            *l = Some(e.time);
            continue;
        }
        *l = Some(e.time);
        out.push(e);
    }
    out
}

fn count_in(ev: &[TimedTeam], from: f64, to: f64) -> [u32; 2] {
    let mut c = [0, 0];
    for e in ev.iter().filter(|e| e.time > from && e.time <= to) {
        c[e.team as usize] += 1;
    }
    c
}

/// Deaths grouped by time and place. Fights need two deaths or more; a lone death is not a fight.
pub fn team_fights(f: &MatchFacts) -> Vec<Fight> {
    // every group whose last death is recent enough stays open; a death joins the nearest open group within
    // the radius, so two fights at once in different lanes stay apart
    let mut groups: Vec<Vec<Death>> = Vec::new();
    for d in &f.deaths {
        let centre = |g: &[Death]| {
            let n = g.len() as f64;
            (
                g.iter().map(|x| x.x).sum::<f64>() / n,
                g.iter().map(|x| x.y).sum::<f64>() / n,
            )
        };
        let best = groups
            .iter()
            .enumerate()
            .filter(|(_, g)| g.last().is_some_and(|x| d.time - x.time <= FIGHT_GAP_S))
            .map(|(i, g)| {
                let (cx, cy) = centre(g);
                (i, (d.x - cx).hypot(d.y - cy))
            })
            .filter(|(_, dist)| *dist <= FIGHT_RADIUS)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i);
        match best {
            Some(i) => groups[i].push(d.clone()),
            None => groups.push(vec![d.clone()]),
        }
    }
    groups.sort_by(|a, b| a[0].time.total_cmp(&b[0].time));
    groups
        .into_iter()
        .filter(|g| g.len() >= 2)
        .map(|deaths| {
            let start = deaths[0].time;
            let end = deaths[deaths.len() - 1].time;
            let mut lost = [0u32; 2];
            for d in &deaths {
                lost[d.team as usize] += 1;
            }
            let winner = match lost[0].cmp(&lost[1]) {
                std::cmp::Ordering::Less => Some(0),
                std::cmp::Ordering::Greater => Some(1),
                std::cmp::Ordering::Equal => None,
            };
            Fight {
                start,
                end,
                lost,
                winner,
                structures_after: count_in(&f.structures, end, end + AFTER_FIGHT_S),
                camps_after: count_in(&f.camps, end, end + AFTER_FIGHT_S),
                objectives_after: count_in(&f.objectives, end, end + AFTER_FIGHT_S),
                deaths,
            }
        })
        .collect()
}

/// Deaths that are not part of any fight (a hero caught or killed on their own).
pub fn isolated_deaths(f: &MatchFacts, fights: &[Fight]) -> Vec<Death> {
    f.deaths
        .iter()
        .filter(|d| {
            !fights.iter().any(|fi| {
                fi.deaths
                    .iter()
                    .any(|x| x.time == d.time && x.victim == d.victim)
            })
        })
        .cloned()
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlayerWindow {
    pub toon: String,
    pub takedowns: u32,
    pub deaths: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TeamWindow {
    pub level_from: u32,
    pub level_to: u32,
    /// XP between the samples at or before `from` and at or after `to` (`None` without samples)
    pub xp: Option<f64>,
    pub structures: u32,
    pub camps: u32,
    pub objectives: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Window {
    pub from: f64,
    pub to: f64,
    /// the XP samples' bounds actually used (whole minutes)
    pub xp_from: Option<f64>,
    pub xp_to: Option<f64>,
    pub players: Vec<PlayerWindow>,
    pub teams: [TeamWindow; 2],
}

fn level_at(times: &[(u32, f64)], t: f64) -> u32 {
    times
        .iter()
        .filter(|(_, x)| *x <= t)
        .map(|(l, _)| *l)
        .max()
        .unwrap_or(0)
}

/// Everything timestamped between `from` (excluded) and `to` (included). Damage is not in it: the replay only
/// stores damage as end-of-game totals.
pub fn window(f: &MatchFacts, toons: &[String], from: f64, to: f64) -> Window {
    let (from, to) = if from <= to { (from, to) } else { (to, from) };
    let in_w = |t: f64| t > from && t <= to;
    let players = toons
        .iter()
        .map(|toon| PlayerWindow {
            toon: toon.clone(),
            takedowns: f
                .deaths
                .iter()
                .filter(|d| in_w(d.time) && d.killers.contains(toon))
                .count() as u32,
            deaths: f
                .deaths
                .iter()
                .filter(|d| in_w(d.time) && &d.victim == toon)
                .count() as u32,
        })
        .collect();
    let s = count_in(&f.structures, from, to);
    let c = count_in(&f.camps, from, to);
    let o = count_in(&f.objectives, from, to);
    let mut xp_from = None;
    let mut xp_to = None;
    let teams = [0usize, 1].map(|t| {
        let samples = &f.xp[t];
        let a = samples.iter().rev().find(|(x, _)| *x <= from).copied();
        let b = samples
            .iter()
            .find(|(x, _)| *x >= to)
            .or(samples.last())
            .copied();
        let xp = match (a, b) {
            (Some(a), Some(b)) if b.0 > a.0 => {
                xp_from = Some(a.0);
                xp_to = Some(b.0);
                Some(b.1 - a.1)
            }
            // before the first sample: count from 0
            (None, Some(b)) => {
                xp_from = Some(0.0);
                xp_to = Some(b.0);
                Some(b.1)
            }
            _ => None,
        };
        TeamWindow {
            level_from: level_at(&f.level_times[t], from),
            level_to: level_at(&f.level_times[t], to),
            xp,
            structures: s[t],
            camps: c[t],
            objectives: o[t],
        }
    });
    Window {
        from,
        to,
        xp_from,
        xp_to,
        players,
        teams,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CampReason {
    TradedForObjective,
    BeforeObjective,
    AfterWonFight,
    Quiet,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Camp {
    pub time: f64,
    pub team: u8,
    #[serde(rename = "type")]
    pub kind: String,
    pub reason: CampReason,
}

/// Why each camp was taken, by timing only (captures carry no position): the enemy took an objective within
/// ±20 s → traded; an objective event within the next 60 s → before an objective; the team won a fight that
/// ended in the 30 s before → after a won fight; otherwise a quiet moment.
pub fn camp_reasons(f: &MatchFacts, fights: &[Fight]) -> Vec<Camp> {
    f.camps
        .iter()
        .map(|c| {
            let reason =
                if f.objectives
                    .iter()
                    .any(|o| o.team != c.team && (o.time - c.time).abs() <= 20.0)
                {
                    CampReason::TradedForObjective
                } else if f
                    .objectives
                    .iter()
                    .any(|o| o.time > c.time && o.time - c.time <= 60.0)
                {
                    CampReason::BeforeObjective
                } else if fights.iter().any(|fi| {
                    fi.winner == Some(c.team) && fi.end <= c.time && c.time - fi.end <= 30.0
                }) {
                    CampReason::AfterWonFight
                } else {
                    CampReason::Quiet
                };
            Camp {
                time: c.time,
                team: c.team,
                kind: c.label.clone(),
                reason,
            }
        })
        .collect()
}

/// Which team got there first, per early lead. `None` = nobody (or unknown).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EarlyLeads {
    pub first_blood: Option<u8>,
    pub first_to_10: Option<u8>,
    pub first_fort: Option<u8>,
    pub first_to_20: Option<u8>,
}

fn first_to(f: &MatchFacts, level: u32) -> Option<u8> {
    let at = |team: usize| {
        f.level_times[team]
            .iter()
            .find(|(l, _)| *l == level)
            .map(|(_, t)| t)
    };
    let (a, b) = (at(0), at(1));
    match (a, b) {
        (Some(a), Some(b)) if a < b => Some(0),
        (Some(a), Some(b)) if b < a => Some(1),
        (Some(_), None) => Some(0),
        (None, Some(_)) => Some(1),
        _ => None,
    }
}

/// `first_fort` = storm-stats `match.firstFort` (the team that destroyed the first fort).
pub fn early_leads(f: &MatchFacts, first_fort: Option<u8>) -> EarlyLeads {
    EarlyLeads {
        first_blood: f.deaths.first().map(|d| 1 - d.team),
        first_to_10: first_to(f, 10),
        first_fort,
        first_to_20: first_to(f, 20),
    }
}

/// Biggest level lead (positive) and biggest deficit (negative, as a positive number) of `team` over the game.
pub fn level_swing(f: &MatchFacts, team: u8) -> (f64, f64) {
    let sign = if team == 0 { 1.0 } else { -1.0 };
    f.level_adv
        .iter()
        .fold((0.0f64, 0.0f64), |(up, down), (_, _, d)| {
            let v = d * sign;
            (up.max(v), down.max(-v))
        })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use serde_json::json;

    fn td(time: f64, x: f64, y: f64, victim: &str, killers: &[&str]) -> J {
        json!({
            "time": time, "x": x * FIXED_POINT, "y": y * FIXED_POINT,
            "victim": {"player": victim, "hero": "H"},
            "killers": killers.iter().map(|k| json!({"player": k, "hero": "K"})).collect::<Vec<_>>()
        })
    }

    fn teams() -> HashMap<String, u8> {
        [
            ("b1", 0),
            ("b2", 0),
            ("b3", 0),
            ("r1", 1),
            ("r2", 1),
            ("r3", 1),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
    }

    fn game() -> J {
        json!({
            "winner": 1,
            "takedowns": [
                td(100.0, 50.0, 50.0, "b1", &["r1", "r2"]),
                td(105.0, 55.0, 52.0, "b2", &["r1"]),
                td(110.0, 52.0, 48.0, "r3", &["b3"]),
                // same time, far away: not the same fight
                td(112.0, 150.0, 150.0, "b3", &["r2"]),
                // 40 s later: a new, lone death
                td(160.0, 52.0, 50.0, "r1", &["b1"]),
            ],
            "structures": {
                "0-1": {"team": 0.0, "name": "Fort", "destroyed": 120.0},
                "1-1": {"team": 1.0, "name": "Fort"}
            },
            "mercs": {"captures": [
                {"time": 125.0, "team": 1.0, "type": "Siege Camp"},
                {"time": 300.0, "team": 0.0, "type": "Bruiser Camp"},
                {"time": 400.0, "team": 0.0, "type": "Siege Camp"}
            ]},
            "objective": {"0": {"events": [{"time": 350.0, "team": 0.0}]},
                          "1": {"events": [{"time": 405.0, "team": 1.0}, {"time": 410.0, "team": 1.0}]},
                          "control": [{"time": 1.0, "team": 1.0}]},
            "levelTimes": {"0": {"1": {"time": -30.0}, "2": {"time": 60.0}, "10": {"time": 500.0}},
                           "1": {"1": {"time": -30.0}, "2": {"time": 55.0}}},
            "XPBreakdown": [
                {"time": 60.0, "team": 0.0, "breakdown": {"MinionXP": 100.0, "TrickleXP": 50.0}},
                {"time": 120.0, "team": 0.0, "breakdown": {"MinionXP": 300.0, "TrickleXP": 100.0}},
                {"time": 60.0, "team": 1.0, "breakdown": {"MinionXP": 120.0}},
                {"time": 120.0, "team": 1.0, "breakdown": {"MinionXP": 260.0}}
            ],
            "levelAdvTimeline": [
                {"start": 0.0, "end": 60.0, "levelDiff": 0.0},
                {"start": 60.0, "end": 200.0, "levelDiff": 2.0},
                {"start": 200.0, "end": 600.0, "levelDiff": -1.0}
            ]
        })
    }

    fn facts() -> MatchFacts {
        MatchFacts::from_match(&game(), &teams())
    }

    #[test]
    fn reads_the_match_facts() {
        let f = facts();
        assert_eq!(f.deaths.len(), 5);
        // the destroyed fort belonged to team 0: destroyed BY team 1
        assert_eq!(f.structures.len(), 1);
        assert_eq!(f.structures[0].team, 1);
        assert_eq!(f.camps.len(), 3);
        // the two red events 5 s apart merge; control samples are ignored
        assert_eq!(f.objectives.len(), 2);
        assert_eq!(f.level_times[0], vec![(1, -30.0), (2, 60.0), (10, 500.0)]);
        assert_eq!(f.xp[0], vec![(60.0, 150.0), (120.0, 400.0)]);
    }

    #[test]
    fn unknown_victims_and_missing_fields_are_skipped() {
        let f = MatchFacts::from_match(
            &json!({"takedowns": [td(1.0, 0.0, 0.0, "zz", &[])]}),
            &teams(),
        );
        assert!(f.deaths.is_empty());
        assert!(team_fights(&f).is_empty());
        let w = window(&f, &[], 0.0, 100.0);
        assert_eq!(w.teams[0].xp, None);
    }

    #[test]
    fn clusters_deaths_by_time_and_place() {
        let f = facts();
        let fights = team_fights(&f);
        assert_eq!(fights.len(), 1, "{fights:?}");
        let fi = &fights[0];
        assert_eq!((fi.start, fi.end), (100.0, 110.0));
        assert_eq!(fi.lost, [2, 1]);
        assert_eq!(fi.winner, Some(1));
        // within 30 s after: the fort (by red) and red's camp
        assert_eq!(fi.structures_after, [0, 1]);
        assert_eq!(fi.camps_after, [0, 1]);
        let alone = isolated_deaths(&f, &fights);
        assert_eq!(
            alone.iter().map(|d| d.victim.as_str()).collect::<Vec<_>>(),
            ["b3", "r1"]
        );
    }

    #[test]
    fn two_fights_at_once_in_different_lanes_stay_apart() {
        let m = json!({"takedowns": [
            td(10.0, 0.0, 0.0, "b1", &[]), td(11.0, 200.0, 0.0, "r1", &[]),
            td(12.0, 1.0, 1.0, "b2", &[]), td(13.0, 201.0, 1.0, "r2", &[])
        ]});
        let fights = team_fights(&MatchFacts::from_match(&m, &teams()));
        assert_eq!(fights.len(), 2, "{fights:?}");
        assert_eq!(fights[0].lost, [2, 0]);
        assert_eq!(fights[1].lost, [0, 2]);
    }

    #[test]
    fn an_even_fight_has_no_winner() {
        let m =
            json!({"takedowns": [td(10.0, 0.0, 0.0, "b1", &[]), td(12.0, 1.0, 1.0, "r1", &[])]});
        let fights = team_fights(&MatchFacts::from_match(&m, &teams()));
        assert_eq!(fights[0].winner, None);
    }

    #[test]
    fn window_counts_what_is_timestamped() {
        let f = facts();
        let toons: Vec<String> = ["b1", "r1"].iter().map(|s| s.to_string()).collect();
        let w = window(&f, &toons, 90.0, 130.0);
        assert_eq!(w.players[0].deaths, 1);
        assert_eq!(w.players[1].takedowns, 2);
        assert_eq!(w.teams[1].structures, 1);
        assert_eq!(w.teams[1].camps, 1);
        // XP snapped to the samples at 60 s and 120 s… `to` = 130 has no later sample: the last one is used
        assert_eq!((w.xp_from, w.xp_to), (Some(60.0), Some(120.0)));
        assert_eq!(w.teams[0].xp, Some(250.0));
        assert_eq!(w.teams[1].xp, Some(140.0));
        assert_eq!((w.teams[0].level_from, w.teams[0].level_to), (2, 2));
        // reversed bounds are swapped
        assert_eq!(window(&f, &toons, 130.0, 90.0).from, 90.0);
    }

    #[test]
    fn camp_reasons_by_timing() {
        let f = facts();
        let fights = team_fights(&f);
        let c = camp_reasons(&f, &fights);
        assert_eq!(c[0].reason, CampReason::AfterWonFight); // red, 15 s after red won the fight
        assert_eq!(c[1].reason, CampReason::BeforeObjective); // blue, 50 s before blue's objective
        assert_eq!(c[2].reason, CampReason::TradedForObjective); // blue, red objective 5 s later
    }

    #[test]
    fn early_leads_and_level_swing() {
        let f = facts();
        let e = early_leads(&f, Some(1));
        assert_eq!(e.first_blood, Some(1));
        assert_eq!(e.first_to_10, Some(0));
        assert_eq!(e.first_to_20, None);
        assert_eq!(e.first_fort, Some(1));
        assert_eq!(level_swing(&f, 0), (2.0, 1.0));
        assert_eq!(level_swing(&f, 1), (1.0, 2.0));
    }
}
