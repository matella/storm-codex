//! Teams: team aggregates, hero uptime and level advantage.

use super::*;

/// Builds `match.teams` (skeleton + per-player) then `collectTeamStats` (2659-2787),
/// `analyzeLevelAdv` (3047-3081), `analyzeUptime` (2789-2926) and the passive XP copies, and
/// copies aces/wipes/timeWithHeroAdv/pctWithHeroAdv into gameStats (2385-2391).
#[allow(clippy::too_many_arguments)]
pub(super) fn build_teams(
    match_: &mut Map<String, J>,
    players: &mut Map<String, J>,
    order: &[String],
    length: f64,
    passive_rate: &[f64; 2],
    passive_diff: &[f64; 2],
    passive_gain: &[f64; 2],
) -> R<()> {
    // ---- match.teams skeleton + per-player filling (parser.js:2132-2191) ----
    let team_td = [
        js_number(match_.get("team0Takedowns")),
        js_number(match_.get("team1Takedowns")),
    ];
    let mut teams: [Map<String, J>; 2] = Default::default();
    for (t, team) in teams.iter_mut().enumerate() {
        team.insert("ids".into(), json!([]));
        team.insert("names".into(), json!([]));
        team.insert("heroes".into(), json!([]));
        team.insert("tags".into(), json!([]));
        team.insert("takedowns".into(), jnum(team_td[t]));
    }
    for handle in order {
        let p = players.get(handle);
        let team = p.and_then(|p| get(p, &["team"]).and_then(J::as_i64));
        let t = match team {
            Some(0) => 0usize,
            Some(1) => 1usize,
            _ => continue, // observers: ignored (parser.js has no branch)
        };
        let level = p
            .and_then(|p| get(p, &["gameStats", "Level"]))
            .cloned()
            .unwrap_or(J::Null);
        let hero = p
            .and_then(|p| get(p, &["hero"]))
            .cloned()
            .unwrap_or(J::Null);
        let name = p
            .and_then(|p| get(p, &["name"]))
            .cloned()
            .unwrap_or(J::Null);
        let tag = p.and_then(|p| get(p, &["tag"])).cloned().unwrap_or(J::Null);
        teams[t].insert("level".into(), level);
        for (field, val) in [("heroes", hero), ("names", name), ("tags", tag)] {
            if let Some(a) = teams[t].get_mut(field).and_then(J::as_array_mut) {
                a.push(val);
            }
        }
        if let Some(a) = teams[t].get_mut("ids").and_then(J::as_array_mut) {
            a.push(J::from(handle.as_str()));
        }
    }
    let team_ids: [Vec<J>; 2] = [
        teams[0]
            .get("ids")
            .and_then(J::as_array)
            .cloned()
            .unwrap_or_default(),
        teams[1]
            .get("ids")
            .and_then(J::as_array)
            .cloned()
            .unwrap_or_default(),
    ];

    // ---- collectTeamStats (parser.js:2659-2787) ----
    let empty = Map::new();
    let mercs_captures = match_
        .get("mercs")
        .and_then(|m| get(m, &["captures"]))
        .and_then(J::as_array)
        .cloned()
        .unwrap_or_default();
    let mercs_units: Vec<J> = match_
        .get("mercs")
        .and_then(|m| get(m, &["units"]))
        .and_then(J::as_object)
        .map(|u| u.values().cloned().collect())
        .unwrap_or_default();
    let structures_all: Vec<J> = match_
        .get("structures")
        .and_then(J::as_object)
        .map(|s| s.values().cloned().collect())
        .unwrap_or_default();
    let takedowns = match_
        .get("takedowns")
        .and_then(J::as_array)
        .cloned()
        .unwrap_or_default();
    let level_times = match_.get("levelTimes").cloned().unwrap_or(J::Null);

    const TOTAL_KEYS: [&str; 16] = [
        "DamageTaken",
        "CreepDamage",
        "Healing",
        "HeroDamage",
        "MinionDamage",
        "SelfHealing",
        "SiegeDamage",
        "ProtectionGivenToAllies",
        "TeamfightDamageTaken",
        "TeamfightHealingDone",
        "TeamfightHeroDamage",
        "TimeCCdEnemyHeroes",
        "TimeRootingEnemyHeroes",
        "TimeSpentDead",
        "TimeStunningEnemyHeroes",
        "TimeSilencingEnemyHeroes",
    ];

    let mut team_stats: [Map<String, J>; 2] = Default::default();
    for t in 0..2 {
        let other = 1 - t;
        let stats = &mut team_stats[t];

        // NB: structure.team / merc.team are floats (0.0/1.0); in JS `0.0 === 0`
        // is true → numeric comparison, not as_i64 (which fails on a float Number).
        let team_eq = |v: Option<&J>, k: usize| js_number(v) == k as f64;

        // merc captures
        let merc_captures = mercs_captures
            .iter()
            .filter(|c| team_eq(get(c, &["team"]), t))
            .count() as i64;
        stats.insert("mercCaptures".into(), J::from(merc_captures));

        // merc uptime (combine intervals, sum of the durations)
        let mut intervals: Vec<(f64, f64)> = Vec::new();
        for unit in &mercs_units {
            if team_eq(get(unit, &["team"]), t) {
                let time = js_number(get(unit, &["time"]));
                let end = if get(unit, &["duration"]).is_none() {
                    length
                } else {
                    time + js_number(get(unit, &["duration"]))
                };
                intervals.push((time, end));
            }
        }
        let merged = combine_intervals(intervals);
        let merc_uptime: f64 = merged.iter().map(|(a, b)| b - a).sum();
        stats.insert("mercUptime".into(), jnum(merc_uptime));
        stats.insert("mercUptimePercent".into(), jnum(merc_uptime / length));

        // structures by name {lost, destroyed, first}
        let mut structures: Map<String, J> = Map::new();
        for s in &structures_all {
            let name = js_prop(get(s, &["name"]));
            if !structures.contains_key(&name) {
                structures.insert(
                    name.clone(),
                    json!({"lost": 0, "destroyed": 0, "first": jnum(length)}),
                );
            }
            if get(s, &["destroyed"]).is_some() {
                let st = get(s, &["team"]);
                let entry = structures.get_mut(&name).and_then(J::as_object_mut);
                if let Some(entry) = entry {
                    if team_eq(st, t) {
                        let v = entry.get("lost").and_then(J::as_i64).unwrap_or(0) + 1;
                        entry.insert("lost".into(), J::from(v));
                    } else if team_eq(st, other) {
                        let v = entry.get("destroyed").and_then(J::as_i64).unwrap_or(0) + 1;
                        entry.insert("destroyed".into(), J::from(v));
                        let first = js_min(
                            js_number(entry.get("first")),
                            js_number(get(s, &["destroyed"])),
                        );
                        entry.insert("first".into(), jnum(first));
                    }
                }
            }
        }
        stats.insert("structures".into(), J::Object(structures));

        // team KDA
        stats.insert("KDA".into(), jnum(team_td[t] / js_max(team_td[other], 1.0)));

        // people per kill
        let mut ppk = 0.0;
        for td in &takedowns {
            let victim = get(td, &["victim", "player"]).cloned().unwrap_or(J::Null);
            if team_ids[other]
                .iter()
                .any(|id| js_strict_eq(Some(id), Some(&victim)))
            {
                ppk += get(td, &["killers"])
                    .and_then(J::as_array)
                    .map_or(0.0, |k| k.len() as f64);
            }
        }
        stats.insert("PPK".into(), jnum(ppk / js_max(team_td[t], 1.0)));

        // timeTo10 / timeTo20 (conditional keys)
        let lt = get(&level_times, &[t.to_string().as_str()]).unwrap_or(&J::Null);
        for lvl in ["10", "20"] {
            if let Some(e) = get(lt, &[lvl]) {
                stats.insert(
                    format!("timeTo{lvl}"),
                    get(e, &["time"]).cloned().unwrap_or(J::Null),
                );
            }
        }

        // totals
        let mut totals: Map<String, J> = Map::new();
        let mut acc = [0.0f64; 16];
        for handle in order {
            let p = players.get(handle);
            if p.and_then(|p| get(p, &["team"]).and_then(J::as_i64)) == Some(t as i64) {
                for (i, k) in TOTAL_KEYS.iter().enumerate() {
                    acc[i] += js_number(p.and_then(|p| get(p, &["gameStats", k])));
                }
            }
        }
        for (i, k) in TOTAL_KEYS.iter().enumerate() {
            totals.insert((*k).into(), jnum(acc[i]));
        }
        let time_spent_dead = acc[13]; // TimeSpentDead
        let avg_dead = time_spent_dead / 5.0;
        totals.insert("avgTimeSpentDead".into(), jnum(avg_dead));
        totals.insert("timeDeadPct".into(), jnum(avg_dead / length));
        stats.insert("totals".into(), J::Object(totals));
        let _ = &empty;
    }

    // ---- analyzeLevelAdv (parser.js:3047-3081) depuis match.levelAdvTimeline ----
    let timeline = match_
        .get("levelAdvTimeline")
        .and_then(J::as_array)
        .cloned()
        .unwrap_or_default();
    let mut adv_time = [0.0f64, 0.0];
    let mut max_adv = [0.0f64, 0.0];
    let mut lvl_avg = [0.0f64, 0.0];
    for lv in &timeline {
        let diff = js_number(get(lv, &["levelDiff"]));
        let len = js_number(get(lv, &["length"]));
        let idx = if diff > 0.0 {
            Some(0)
        } else if diff < 0.0 {
            Some(1)
        } else {
            None
        };
        if let Some(i) = idx {
            adv_time[i] += len;
            lvl_avg[i] += len * diff.abs();
            if diff.abs() > max_adv[i] {
                max_adv[i] = diff.abs();
            }
        }
    }
    for t in 0..2 {
        team_stats[t].insert("levelAdvTime".into(), jnum(adv_time[t]));
        team_stats[t].insert("maxLevelAdv".into(), jnum(max_adv[t]));
        team_stats[t].insert("avgLevelAdv".into(), jnum(lvl_avg[t] / length));
        team_stats[t].insert("levelAdvPct".into(), jnum(adv_time[t] / length));
    }

    // ---- analyzeUptime (parser.js:2789-2926) ----
    let uptime = [
        analyze_team_uptime(0, players, order),
        analyze_team_uptime(1, players, order),
    ];
    let lifespan_json = |tl: &[(f64, f64)]| -> J {
        J::Array(
            tl.iter()
                .map(|(t, h)| json!({"time": jnum(*t), "heroes": jnum(*h)}))
                .collect(),
        )
    };
    let histo_json = |hc: &[(f64, f64)]| -> J {
        // heroCount: object {str: duration}, keys in order of first appearance
        let mut m = Map::new();
        for (s, d) in hc {
            m.insert(js_prop(Some(&jnum(*s))), jnum(*d));
        }
        J::Object(m)
    };
    for t in 0..2 {
        let u = &uptime[t];
        team_stats[t].insert("uptime".into(), lifespan_json(&u.team_lifespan));
        team_stats[t].insert("uptimeHistogram".into(), histo_json(&u.hero_count));
        team_stats[t].insert("wipes".into(), J::from(u.wipes));
        team_stats[t].insert("avgHeroesAlive".into(), jnum(u.avg_heroes_alive));
        team_stats[t].insert("aces".into(), J::from(uptime[1 - t].wipes)); // aces = wipes adverse
    }
    for t in 0..2 {
        let twha = time_with_hero_adv(
            &uptime[t].team_lifespan,
            &uptime[1 - t].team_lifespan,
            length,
        );
        team_stats[t].insert("timeWithHeroAdv".into(), jnum(twha));
        team_stats[t].insert("pctWithHeroAdv".into(), jnum(twha / length));
    }

    // ---- passive XP → stats (parser.js:2357-2375) ----
    for t in 0..2 {
        team_stats[t].insert("passiveXPRate".into(), jnum(passive_rate[t]));
        team_stats[t].insert("passiveXPDiff".into(), jnum(passive_diff[t]));
        team_stats[t].insert("passiveXPGain".into(), jnum(passive_gain[t]));
    }

    // puts the stats into the teams then into match.teams
    let mut teams_map = Map::new();
    for (t, mut team) in teams.into_iter().enumerate() {
        team.insert(
            "stats".into(),
            J::Object(std::mem::take(&mut team_stats[t])),
        );
        teams_map.insert(t.to_string(), J::Object(team));
    }
    match_.insert("teams".into(), J::Object(teams_map));

    // ---- copies of aces/wipes/timeWithHeroAdv/pctWithHeroAdv into gameStats (2385-2388) ----
    for handle in order {
        let team = players
            .get(handle)
            .and_then(|p| get(p, &["team"]).and_then(J::as_i64));
        let t = match team {
            Some(0) => 0usize,
            Some(1) => 1usize,
            _ => return Err(Abort::Throw("teamStats copy: invalid team".into())),
        };
        let copy: Vec<(String, J)> = ["aces", "wipes", "timeWithHeroAdv", "pctWithHeroAdv"]
            .iter()
            .map(|k| ((*k).into(), team_stats_value(match_, t, k)))
            .collect();
        let gs = players
            .get_mut(handle)
            .and_then(J::as_object_mut)
            .and_then(|p| p.get_mut("gameStats"))
            .and_then(J::as_object_mut)
            .ok_or_else(|| Abort::Throw("teamStats copie: gameStats absent".into()))?;
        for (k, v) in copy {
            gs.insert(k, v);
        }
    }
    Ok(())
}

pub(super) fn team_stats_value(match_: &Map<String, J>, t: usize, key: &str) -> J {
    match_
        .get("teams")
        .and_then(|teams| get(teams, &[t.to_string().as_str(), "stats", key]))
        .cloned()
        .unwrap_or(J::Null)
}

pub(super) struct TeamUptime {
    pub(super) team_lifespan: Vec<(f64, f64)>, // (time, heroes)
    pub(super) hero_count: Vec<(f64, f64)>,    // (heroes, duration), order of first appearance
    pub(super) wipes: i64,
    pub(super) avg_heroes_alive: f64,
}

/// `analyzeTeamPlayerUptime` (parser.js:2849-2926).
pub(super) fn analyze_team_uptime(
    team: i64,
    players: &Map<String, J>,
    order: &[String],
) -> TeamUptime {
    let mut events: Vec<(f64, f64)> = Vec::new(); // (time, str)
    let mut match_length = 0.0;
    for handle in order {
        let p = players.get(handle);
        if p.and_then(|p| get(p, &["team"]).and_then(J::as_i64)) != Some(team) {
            continue;
        }
        match_length = js_number(p.and_then(|p| get(p, &["length"])));
        let lifespan = p
            .and_then(|p| get(p, &["lifespan"]))
            .and_then(J::as_array)
            .cloned()
            .unwrap_or_default();
        for life in &lifespan {
            let l0 = js_number(get(life, &["0"]));
            let l1 = js_number(get(life, &["1"]));
            if l0 > 0.0 {
                events.push((l0, 1.0));
            }
            if l1 != match_length {
                events.push((l1, -1.0));
            }
        }
    }
    events.sort_by(|a, b| js_cmp(a.0, b.0));

    let mut team_lifespan = vec![(0.0f64, 5.0f64)];
    let mut current = 5.0;
    for (time, str_) in &events {
        current += str_;
        team_lifespan.push((*time, current));
    }

    let mut hero_count: Vec<(f64, f64)> = Vec::new();
    let mut wipes = 0i64;
    let mut avg = 0.0;
    for i in 0..team_lifespan.len() {
        let next_time = if i + 1 >= team_lifespan.len() {
            match_length
        } else {
            team_lifespan[i + 1].0
        };
        let dur = next_time - team_lifespan[i].0;
        let str_ = team_lifespan[i].1;
        match hero_count.iter_mut().find(|(s, _)| *s == str_) {
            Some(e) => e.1 += dur,
            None => hero_count.push((str_, dur)),
        }
        if str_ == 0.0 {
            wipes += 1;
        }
        avg += str_ * dur;
    }
    TeamUptime {
        team_lifespan,
        hero_count,
        wipes,
        avg_heroes_alive: avg / match_length,
    }
}

/// `timeWithHeroAdv` (parser.js:2928-2960).
pub(super) fn time_with_hero_adv(
    base: &[(f64, f64)],
    compare: &[(f64, f64)],
    match_length: f64,
) -> f64 {
    let mut xs: Vec<f64> = base.iter().map(|(t, _)| *t).collect();
    xs.extend(compare.iter().map(|(t, _)| *t));
    xs.sort_by(|a, b| js_cmp(*a, *b));
    let str_at = |data: &[(f64, f64)], time: f64| -> f64 {
        let mut s = 0.0;
        for (t, h) in data {
            if *t <= time {
                s = *h;
            }
        }
        s
    };
    let mut adv = 0.0;
    for i in 0..xs.len() {
        if str_at(base, xs[i]) > str_at(compare, xs[i]) {
            adv += if i + 1 >= xs.len() {
                match_length - xs[i]
            } else {
                xs[i + 1] - xs[i]
            };
        }
    }
    adv
}
