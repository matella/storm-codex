//! Match detail page: pure rules over the rich storm-stats JSON (`GET /api/matches/:id`), kept as `serde_json::Value`
//! like the untyped `any` of the React page. Port of the logic of `web/src/pages/MatchDetail.tsx` and of
//! `LevelAdvantage` (`components/ds/match.tsx`).
//!
//! The object key order is the JSON order (`preserve_order`): the scoreboard lists players the way the server wrote them.

use serde_json::Value;

use crate::session::{fmt_dur, to_fixed1};

static NULL: Value = Value::Null;

/// `v[key]`, `null` when missing.
pub fn get<'a>(v: &'a Value, key: &str) -> &'a Value {
    v.get(key).unwrap_or(&NULL)
}

/// `v[i]` of an array or `v["i"]` of an object (the server uses both for the two teams).
pub fn idx(v: &Value, i: usize) -> &Value {
    match v {
        Value::Array(a) => a.get(i).unwrap_or(&NULL),
        Value::Object(o) => o.get(&i.to_string()).unwrap_or(&NULL),
        _ => &NULL,
    }
}

/// `num`: the number, 0 for anything else.
pub fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0)
}

/// The value as an array (empty when it is not one).
pub fn arr(v: &Value) -> &[Value] {
    v.as_array().map_or(&[], Vec::as_slice)
}

/// A string field, `None` when missing or not a string.
pub fn text(v: &Value) -> Option<&str> {
    v.as_str()
}

/// `v === n` for a JSON number (the parser writes `0.0` for team numbers).
pub fn is_num(v: &Value, n: f64) -> bool {
    v.as_f64() == Some(n)
}

/// Present and not `null` (`!= null`).
pub fn some(v: &Value) -> bool {
    !v.is_null()
}

/// JS truthiness of a JSON value.
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

/// `Math.round`: halves go up.
pub fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// `fmtK`: `1.2k` from a thousand, the rounded number below.
pub fn fmt_k(n: f64) -> String {
    if n >= 1000.0 {
        format!("{}k", to_fixed1(n / 1000.0))
    } else {
        format!("{}", js_round(n))
    }
}

/// `fmtClock`: signed `m:ss` (negative times are before the start of the game).
pub fn fmt_clock(sec: f64) -> String {
    let s = sec.abs().floor() as i64;
    format!(
        "{}{}:{:02}",
        if sec < 0.0 { "−" } else { "" },
        s / 60,
        s % 60
    )
}

/// `announceLabel` of a callout message.
pub fn announce_label(a: &Value) -> &'static str {
    match a
        .as_object()
        .and_then(|o| o.keys().next())
        .map(String::as_str)
    {
        Some("Ability") => "ability callout",
        Some("Vitals") => "vitals callout (help)",
        Some("Behavior") => "behavior callout",
        _ => "callout",
    }
}

pub const MSG_CHAT: i64 = 0;
pub const MSG_PING: i64 = 1;
pub const MSG_ANNOUNCE: i64 = 5;

/// Recipient of a chat message or ping.
pub fn target(recipient: &Value) -> Option<&'static str> {
    match recipient.as_f64() {
        Some(0.0) => Some("all"),
        Some(1.0) => Some("allies"),
        Some(4.0) => Some("obs"),
        _ => None,
    }
}

/// Scoreboard column: where the value comes from and how it is shown.
pub struct Col {
    pub label: &'static str,
    /// Key of `gameStats` (`None` for APM, read from the player).
    pub key: Option<&'static str>,
    /// Extended columns, hidden by default.
    pub adv: bool,
    /// Summed in the "Team total" row.
    pub total: bool,
    /// Shown as a gauge relative to the maximum of the game.
    pub bar: bool,
}

pub const COLS: [Col; 11] = [
    Col {
        label: "Hero dmg",
        key: Some("HeroDamage"),
        adv: false,
        total: true,
        bar: true,
    },
    Col {
        label: "Siege",
        key: Some("SiegeDamage"),
        adv: false,
        total: true,
        bar: true,
    },
    Col {
        label: "Healing",
        key: Some("Healing"),
        adv: false,
        total: true,
        bar: true,
    },
    Col {
        label: "XP",
        key: Some("ExperienceContribution"),
        adv: false,
        total: true,
        bar: true,
    },
    Col {
        label: "Lvl",
        key: Some("Level"),
        adv: false,
        total: false,
        bar: false,
    },
    Col {
        label: "APM",
        key: None,
        adv: false,
        total: false,
        bar: false,
    },
    Col {
        label: "Spell",
        key: Some("SpellDamage"),
        adv: true,
        total: true,
        bar: true,
    },
    Col {
        label: "Taken",
        key: Some("DamageTaken"),
        adv: true,
        total: true,
        bar: true,
    },
    Col {
        label: "Self-heal",
        key: Some("SelfHealing"),
        adv: true,
        total: true,
        bar: false,
    },
    Col {
        label: "CC s",
        key: Some("TimeCCdEnemyHeroes"),
        adv: true,
        total: true,
        bar: false,
    },
    Col {
        label: "Mercs",
        key: Some("MercCampCaptures"),
        adv: true,
        total: true,
        bar: false,
    },
];

/// Value of a column for a player (`gameStats` is `g`, the player itself `p`).
pub fn col_value(c: &Col, g: &Value, p: &Value) -> f64 {
    match c.key {
        Some(k) => num(get(g, k)),
        None => num(get(p, "apm")),
    }
}

/// Maximum of each column over the players (at least 1), for the gauges.
pub fn col_maxes(players: &[&Value]) -> Vec<f64> {
    COLS.iter()
        .map(|c| {
            players
                .iter()
                .map(|p| col_value(c, get(p, "gameStats"), p))
                .fold(1.0, f64::max)
        })
        .collect()
}

/// Width of a gauge in percent.
pub fn bar_width(v: f64, max: f64) -> f64 {
    js_round(v * 100.0 / if max == 0.0 { 1.0 } else { max })
}

/// `Assists ?? Takedowns` of the K/D/A cell.
pub fn assists(g: &Value) -> f64 {
    if some(get(g, "Assists")) {
        num(get(g, "Assists"))
    } else {
        num(get(g, "Takedowns"))
    }
}

/// Objective noun by map (the `objective.type` field is the map name).
fn objective_noun(map: &str) -> &'static str {
    match map {
        "Dragon Shire" => "Dragon Knight",
        "Garden of Terror" => "Garden Terror",
        "Cursed Hollow" => "Curse",
        "Battlefield of Eternity" => "Immortal",
        "Tomb of the Spider Queen" => "Webweaver",
        "Sky Temple" => "Temple",
        "Towers of Doom" => "Altar",
        "Braxis Holdout" => "Zerg wave",
        "Volskaya Foundry" => "Triglav",
        "Infernal Shrines" => "Punisher",
        "Hanamura Temple" => "Payload",
        "Blackheart's Bay" => "Cannons",
        "Alterac Pass" => "Cavalry",
        _ => "Objective",
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ev {
    pub t: f64,
    pub kind: EvKind,
    /// Team that scores (opposite to the victim or to the owner of the structure).
    pub team: i64,
    pub label: String,
    pub hero: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvKind {
    Kill,
    Struct,
    Obj,
}

/// Timed objective events: `results` (Battlefield) or the `0` / `1` buckets.
fn objective_events(m: &Value) -> Vec<Ev> {
    let o = get(m, "objective");
    if !o.is_object() {
        return Vec::new();
    }
    let noun = objective_noun(text(get(o, "type")).unwrap_or(""));
    let mut out = Vec::new();
    let ev = |t: f64, team: i64| Ev {
        t,
        kind: EvKind::Obj,
        team,
        label: noun.to_string(),
        hero: None,
    };
    for r in arr(get(o, "results")) {
        let w = get(r, "winner");
        if some(get(r, "time")) && (is_num(w, 0.0) || is_num(w, 1.0)) {
            out.push(ev(num(get(r, "time")), num(w) as i64));
        }
    }
    for k in ["0", "1"] {
        for e in arr(get(get(o, k), "events")) {
            if some(get(e, "time")) {
                out.push(ev(num(get(e, "time")), k.parse().unwrap_or(0)));
            }
        }
    }
    out
}

/// Kills, destroyed structures and objectives in time order (stable).
pub fn timeline_events(m: &Value, players: &Value) -> Vec<Ev> {
    let mut evs = Vec::new();
    for td in arr(get(m, "takedowns")) {
        let victim = get(td, "victim");
        let team = text(get(victim, "player")).map(|t| get(get(players, t), "team"));
        let killers = arr(get(td, "killers")).len();
        let hero = text(get(victim, "hero")).map(str::to_string);
        evs.push(Ev {
            t: num(get(td, "time")),
            kind: EvKind::Kill,
            team: if team.is_some_and(|t| is_num(t, 0.0)) {
                1
            } else {
                0
            },
            label: format!(
                "{} killed{}",
                hero.as_deref().unwrap_or("?"),
                if killers > 1 {
                    format!(" ×{killers}")
                } else {
                    String::new()
                }
            ),
            hero,
        });
    }
    if let Some(structs) = get(m, "structures").as_object() {
        for s in structs.values() {
            if !some(get(s, "destroyed")) {
                continue;
            }
            evs.push(Ev {
                t: num(get(s, "destroyed")),
                kind: EvKind::Struct,
                team: if is_num(get(s, "team"), 0.0) { 1 } else { 0 },
                label: format!("{} destroyed", text(get(s, "name")).unwrap_or("undefined")),
                hero: None,
            });
        }
    }
    evs.extend(objective_events(m));
    evs.sort_by(|a, b| a.t.total_cmp(&b.t));
    evs
}

/// Length of the timeline axis: the game length or the last event, 1 at least.
pub fn timeline_max(length: f64, evs: &[Ev]) -> f64 {
    let m = evs.iter().map(|e| e.t).fold(length, f64::max);
    if m == 0.0 {
        1.0
    } else {
        m
    }
}

/// Team XP polylines (`points` of the blue and red team): the sum of the numeric breakdown fields that are not times.
pub fn xp_lines(data: &[Value]) -> Option<(String, String)> {
    if data.len() < 2 {
        return None;
    }
    let sum = |b: &Value| -> f64 {
        b.as_object().map_or(0.0, |o| {
            o.iter()
                .filter(|(k, v)| v.is_number() && !k.to_lowercase().contains("time"))
                .map(|(_, v)| num(v))
                .sum()
        })
    };
    let mut series: [Vec<(f64, f64)>; 2] = [Vec::new(), Vec::new()];
    for d in data {
        let t = if is_num(get(d, "team"), 0.0) { 0 } else { 1 };
        series[t].push((num(get(d, "time")), sum(get(d, "breakdown"))));
    }
    for s in &mut series {
        s.sort_by(|a, b| a.0.total_cmp(&b.0));
    }
    let max_xp = data
        .iter()
        .map(|d| sum(get(d, "breakdown")))
        .fold(1.0, f64::max);
    let max_t = data.iter().map(|d| num(get(d, "time"))).fold(1.0, f64::max);
    let line = |pts: &[(f64, f64)]| {
        pts.iter()
            .map(|(t, xp)| format!("{},{}", (t / max_t) * 100.0, 100.0 - (xp / max_xp) * 100.0))
            .collect::<Vec<_>>()
            .join(" ")
    };
    Some((line(&series[0]), line(&series[1])))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    pub t: f64,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LevelAdvSvg {
    pub w: f64,
    pub h: f64,
    pub mid: f64,
    pub d: String,
    /// `x` of the two-minute ticks.
    pub ticks: Vec<f64>,
    /// `x` of each mark and the mark.
    pub marks: Vec<(f64, Mark)>,
    /// `x`, width and tooltip of each segment.
    pub rects: Vec<(f64, f64, String)>,
    pub length: f64,
}

/// `LevelAdvantage`: the level difference as a staircase; above the line is `side` ahead (0 = blue team, the parser's
/// difference being blue minus red).
pub fn level_adv(
    timeline: &[Value],
    length: f64,
    side: i64,
    marks: &[Mark],
) -> Option<LevelAdvSvg> {
    if timeline.is_empty() || length == 0.0 {
        return None;
    }
    let (w, h) = (1000.0, 160.0);
    let mid = h / 2.0;
    let max_abs = timeline
        .iter()
        .map(|s| num(get(s, "levelDiff")).abs())
        .fold(2.0, f64::max);
    let k = (mid - 12.0) / max_abs;
    let sgn = if side == 0 { 1.0 } else { -1.0 };
    let mut pts = vec![(0.0, mid)];
    for s in timeline {
        let y = mid - sgn * num(get(s, "levelDiff")) * k;
        pts.push(((num(get(s, "start")) / length) * w, y));
        pts.push(((num(get(s, "end")) / length) * w, y));
    }
    pts.push((w, mid));
    let d = format!(
        "M{}",
        pts.iter()
            .map(|(x, y)| format!("{},{}", to_fixed1(*x), to_fixed1(*y)))
            .collect::<Vec<_>>()
            .join(" L")
    );
    let mut ticks = Vec::new();
    let mut t = 120.0;
    while t < length {
        ticks.push((t / length) * w);
        t += 120.0;
    }
    let rects = timeline
        .iter()
        .map(|s| {
            let (start, end, diff) = (
                num(get(s, "start")),
                num(get(s, "end")),
                num(get(s, "levelDiff")),
            );
            let what = if diff == 0.0 {
                "even".to_string()
            } else {
                format!(
                    "{} level{} {}",
                    diff.abs(),
                    if diff.abs() > 1.0 { "s" } else { "" },
                    if sgn * diff > 0.0 { "ahead" } else { "behind" }
                )
            };
            (
                (start / length) * w,
                (((end - start) / length) * w).max(1.0),
                format!("{}–{} · {what}", fmt_dur(Some(start)), fmt_dur(Some(end))),
            )
        })
        .collect();
    Some(LevelAdvSvg {
        w,
        h,
        mid,
        d,
        ticks,
        marks: marks
            .iter()
            .map(|m| ((m.t / length) * w, m.clone()))
            .collect(),
        rects,
        length,
    })
}

/// One line of a scoreboard.
#[derive(Clone, Debug, PartialEq)]
pub struct ScoreRow {
    pub toon: String,
    pub hero: String,
    pub name: String,
    pub award: Option<String>,
    pub kills: f64,
    pub deaths: f64,
    pub assists: f64,
    /// Value of each column of `COLS`.
    pub vals: Vec<f64>,
    /// Talent tree ids in level order; `None` when the player has no `talents` object.
    pub talents: Option<Vec<String>>,
}

/// The players of a team as scoreboard lines, in server order.
pub fn score_rows(players: &[&Value], team: i64) -> Vec<ScoreRow> {
    players
        .iter()
        .filter(|p| is_num(get(p, "team"), team as f64))
        .map(|p| {
            let g = get(p, "gameStats");
            ScoreRow {
                toon: text(get(p, "ToonHandle")).unwrap_or_default().to_string(),
                hero: text(get(p, "hero")).unwrap_or_default().to_string(),
                name: text(get(p, "name")).unwrap_or_default().to_string(),
                award: arr(get(g, "awards"))
                    .first()
                    .and_then(|a| text(a))
                    .map(str::to_string),
                kills: num(get(g, "SoloKill")),
                deaths: num(get(g, "Deaths")),
                assists: assists(g),
                vals: COLS.iter().map(|c| col_value(c, g, p)).collect(),
                talents: some(get(p, "talents")).then(|| talent_picks(get(p, "talents"))),
            }
        })
        .collect()
}

/// Name of a draft ban (`{hero}` or a bare attribute code): the reference name when known, the code as it is
/// otherwise, `None` for an empty slot.
pub fn ban_hero(ban: &Value, attrs: &std::collections::HashMap<String, String>) -> Option<String> {
    let code = match ban {
        Value::String(s) => s.as_str(),
        v => text(get(v, "hero"))?,
    };
    if code.is_empty() {
        return None;
    }
    Some(attrs.get(code).cloned().unwrap_or_else(|| code.to_string()))
}

/// Phase of a ban: `MID` / `1ST` for the ordered ones, `BAN` for a bare code.
pub fn ban_phase(ban: &Value) -> &'static str {
    if ban.is_object() {
        if is_num(get(ban, "order"), 2.0) {
            "MID"
        } else {
            "1ST"
        }
    } else {
        "BAN"
    }
}

/// Last `Tier\d` keys of a player's talents as tree ids in level order.
pub fn talent_picks(talents: &Value) -> Vec<String> {
    let map: std::collections::BTreeMap<String, String> = talents
        .as_object()
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();
    crate::herodetail::build_picks(&map)
}

/// Number of entries of an array field (0 for anything else).
pub fn count(v: &Value) -> usize {
    v.as_array().map_or(0, Vec::len)
}

/// Pings of a player in `match.messages`.
pub fn pings(messages: &Value, toon: &str) -> usize {
    arr(messages)
        .iter()
        .filter(|x| text(get(x, "player")) == Some(toon) && num_eq(get(x, "type"), MSG_PING))
        .count()
}

/// `type` of a message equals `n`.
pub fn num_eq(v: &Value, n: i64) -> bool {
    is_num(v, n as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn numbers_print_like_the_react_page() {
        assert_eq!(fmt_k(999.4), "999");
        assert_eq!(fmt_k(1250.0), "1.3k");
        assert_eq!(fmt_k(0.5), "1");
        assert_eq!(fmt_clock(-51.0), "−0:51");
        assert_eq!(fmt_clock(75.9), "1:15");
        assert_eq!(bar_width(50.0, 200.0), 25.0);
        assert_eq!(bar_width(1.0, 3.0), 33.0);
    }

    #[test]
    fn timeline_scores_for_the_team_that_did_not_lose() {
        let players = json!({"a": {"team": 0}, "b": {"team": 1}});
        let m = json!({
            "takedowns": [
                {"time": 20.0, "victim": {"hero": "Zarya", "player": "a"}, "killers": [{}, {}]},
                {"time": 5.0, "victim": {"hero": "Nova", "player": "b"}, "killers": [{}]}
            ],
            "structures": {"x": {"destroyed": 100.0, "team": 1.0, "name": "Fort"}, "y": {"team": 0.0}},
            "objective": {"type": "Sky Temple", "0": {"events": [{"time": 60.0}]}}
        });
        let evs = timeline_events(&m, &players);
        let line: Vec<_> = evs
            .iter()
            .map(|e| (e.t, e.team, e.label.as_str()))
            .collect();
        assert_eq!(
            line,
            [
                (5.0, 0, "Nova killed"),
                (20.0, 1, "Zarya killed ×2"),
                (60.0, 0, "Temple"),
                (100.0, 0, "Fort destroyed"),
            ]
        );
        assert_eq!(timeline_max(90.0, &evs), 100.0);
        assert_eq!(timeline_max(0.0, &[]), 1.0);
    }

    #[test]
    fn team_xp_skips_the_time_fields() {
        let data = [
            json!({"team": 0.0, "time": 60.0, "breakdown": {"HeroXP": 100.0, "GameTime": 60.0}}),
            json!({"team": 1.0, "time": 60.0, "breakdown": {"MinionXP": 50.0}}),
        ];
        let (blue, red) = xp_lines(&data).unwrap();
        assert_eq!(blue, "100,0");
        assert_eq!(red, "100,50");
        assert!(xp_lines(&data[..1]).is_none());
    }

    #[test]
    fn the_level_staircase_goes_up_for_the_side_that_is_ahead() {
        let tl = [
            json!({"start": 0.0, "end": 60.0, "levelDiff": 0.0}),
            json!({"start": 60.0, "end": 120.0, "levelDiff": 1.0}),
        ];
        let blue = level_adv(&tl, 240.0, 0, &[]).unwrap();
        assert_eq!(
            blue.d,
            "M0.0,80.0 L0.0,80.0 L250.0,80.0 L250.0,46.0 L500.0,46.0 L1000.0,80.0"
        );
        let red = level_adv(&tl, 240.0, 1, &[]).unwrap();
        assert!(red.d.contains("250.0,114.0"));
        assert_eq!(blue.ticks, [500.0]);
        assert_eq!(blue.rects[1].2, "1:00–2:00 · 1 level ahead");
        assert_eq!(red.rects[1].2, "1:00–2:00 · 1 level behind");
        assert!(level_adv(&[], 240.0, 0, &[]).is_none());
        assert!(level_adv(&tl, 0.0, 0, &[]).is_none());
    }

    #[test]
    fn bans_resolve_through_the_attribute_codes() {
        let attrs = [("Crus".to_string(), "Johanna".to_string())]
            .into_iter()
            .collect();
        assert_eq!(ban_hero(&json!("Crus"), &attrs).as_deref(), Some("Johanna"));
        assert_eq!(
            ban_hero(&json!({"hero": "XXXX", "order": 2}), &attrs).as_deref(),
            Some("XXXX")
        );
        assert_eq!(ban_hero(&json!({"hero": ""}), &attrs), None);
        assert_eq!(ban_hero(&json!({}), &attrs), None);
        assert_eq!(ban_phase(&json!({"order": 2})), "MID");
        assert_eq!(ban_phase(&json!({"order": 1})), "1ST");
        assert_eq!(ban_phase(&json!("Crus")), "BAN");
    }

    #[test]
    fn the_two_team_containers_are_read_by_index_or_key() {
        assert_eq!(idx(&json!(["a", "b"]), 1), &json!("b"));
        assert_eq!(idx(&json!({"0": "a", "1": "b"}), 1), &json!("b"));
        assert_eq!(idx(&json!(null), 1), &Value::Null);
        assert_eq!(
            announce_label(&json!({"Vitals": {}})),
            "vitals callout (help)"
        );
        assert_eq!(target(&json!(4)), Some("obs"));
        assert_eq!(target(&json!(9)), None);
    }

    #[test]
    fn score_rows_follow_the_team_and_the_server_order() {
        let a = json!({"team": 0.0, "ToonHandle": "t1", "hero": "Zarya", "name": "A", "apm": 100.0,
            "gameStats": {"SoloKill": 3, "Deaths": 1, "Takedowns": 7, "HeroDamage": 500.0, "awards": ["EndOfMatchAwardMVPBoolean"]},
            "talents": {"Tier2Choice": "b", "Tier1Choice": "a"}});
        let b = json!({"team": 1.0, "ToonHandle": "t2", "hero": "Nova", "name": "B"});
        let c = json!({"team": 0.0, "ToonHandle": "t3", "hero": "Tracer", "name": "C", "gameStats": {}});
        let rows = score_rows(&[&a, &b, &c], 0);
        assert_eq!(
            rows.iter().map(|r| r.toon.as_str()).collect::<Vec<_>>(),
            ["t1", "t3"]
        );
        assert_eq!(
            (rows[0].kills, rows[0].deaths, rows[0].assists),
            (3.0, 1.0, 7.0)
        );
        assert_eq!(rows[0].vals[0], 500.0);
        assert_eq!(rows[0].vals[5], 100.0);
        assert_eq!(rows[0].award.as_deref(), Some("EndOfMatchAwardMVPBoolean"));
        assert_eq!(rows[0].talents, Some(vec!["a".into(), "b".into()]));
        assert_eq!(rows[1].talents, None);
        assert_eq!(col_maxes(&[&a, &b])[0], 500.0);
        assert_eq!(col_maxes(&[&b])[0], 1.0);
    }

    #[test]
    fn assists_fall_back_to_takedowns_only_when_missing() {
        assert_eq!(assists(&json!({"Assists": 0, "Takedowns": 5})), 0.0);
        assert_eq!(assists(&json!({"Takedowns": 5})), 5.0);
    }
}
