//! Local insights (spec `docs/specs/2026-11-17-local-insights-design.md`): wire models of the
//! `/api/matches/{id}/insights`, `/api/records`, `/api/maps/early-leads`, `/api/compare` and
//! `/api/players/{toon}/insights` answers, and the small pure helpers their pages use.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Death {
    pub time: f64,
    pub team: u8,
    pub victim: String,
    #[serde(default)]
    pub victim_hero: String,
    #[serde(default)]
    pub killers: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Fight {
    pub start: f64,
    pub end: f64,
    pub lost: [u32; 2],
    pub winner: Option<u8>,
    #[serde(default)]
    pub deaths: Vec<Death>,
    pub structures_after: [u32; 2],
    pub camps_after: [u32; 2],
    pub objectives_after: [u32; 2],
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Camp {
    pub time: f64,
    pub team: u8,
    #[serde(rename = "type")]
    pub kind: String,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct PlayerWindow {
    pub toon: String,
    pub takedowns: u32,
    pub deaths: u32,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct TeamWindow {
    pub level_from: u32,
    pub level_to: u32,
    pub xp: Option<f64>,
    pub structures: u32,
    pub camps: u32,
    pub objectives: u32,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Window {
    pub from: f64,
    pub to: f64,
    pub xp_from: Option<f64>,
    pub xp_to: Option<f64>,
    pub players: Vec<PlayerWindow>,
    pub teams: [TeamWindow; 2],
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct TeamfightTotal {
    pub toon: String,
    pub hero_damage: Option<f64>,
    pub damage_taken: Option<f64>,
    pub healing: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct MatchInsights {
    pub fights: Vec<Fight>,
    pub camps: Vec<Camp>,
    #[serde(default)]
    pub isolated_deaths: Vec<Death>,
    pub window: Option<Window>,
    #[serde(default)]
    pub teamfight_totals: Vec<TeamfightTotal>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RecordRow {
    pub value: f64,
    pub toon: String,
    pub name: Option<String>,
    pub hero: Option<String>,
    pub map: Option<String>,
    pub played_at: Option<String>,
    pub match_id: i64,
    #[serde(default)]
    pub mine: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RecordStat {
    pub key: String,
    pub label: String,
    pub rows: Vec<RecordRow>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Records {
    pub stats: Vec<RecordStat>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Lead {
    pub key: String,
    pub label: String,
    pub games: u32,
    pub wins: u32,
    pub my_games: u32,
    pub my_wins: u32,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct MapLeads {
    pub map: String,
    pub games: u32,
    pub leads: Vec<Lead>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct EarlyLeads {
    pub games: u32,
    pub leads: Vec<Lead>,
    pub maps: Vec<MapLeads>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct HeroCount {
    pub hero: String,
    pub games: u32,
    pub wins: u32,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Card {
    pub toon: String,
    pub name: Option<String>,
    pub games: u32,
    pub wins: u32,
    pub kills: Option<f64>,
    pub deaths: Option<f64>,
    pub assists: Option<f64>,
    #[serde(default)]
    pub heroes: Vec<HeroCount>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Together {
    pub games: u32,
    pub wins: u32,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Against {
    pub games: u32,
    pub a_wins: u32,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct SharedGame {
    pub match_id: i64,
    pub map: Option<String>,
    pub played_at: Option<String>,
    pub together: bool,
    pub a_hero: Option<String>,
    pub b_hero: Option<String>,
    pub a_win: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Compare {
    pub a: Card,
    pub b: Card,
    pub together: Together,
    pub against: Against,
    pub shared: Vec<SharedGame>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Nemesis {
    pub toon: String,
    pub name: Option<String>,
    pub kills: u32,
    pub games: u32,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Ranked {
    pub name: String,
    pub games: u32,
    pub wins: u32,
    pub pct: f64,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Count {
    #[serde(default)]
    pub losses: u32,
    #[serde(default)]
    pub wins: u32,
    pub count: u32,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct DeathCount {
    pub total: u32,
    pub isolated: u32,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct PlayerInsights {
    pub games: u32,
    pub small_sample: bool,
    pub nemesis: Vec<Nemesis>,
    pub thrown: Count,
    pub comebacks: Count,
    pub deaths: DeathCount,
    #[serde(default)]
    pub maps: Vec<Ranked>,
    #[serde(default)]
    pub heroes: Vec<Ranked>,
}

/// Rounded percentage, 0 when there is nothing to divide.
pub fn pct(n: u32, d: u32) -> u32 {
    if d == 0 {
        0
    } else {
        ((n as f64) * 100.0 / d as f64).round() as u32
    }
}

/// `m:ss` of game seconds (negative = before the gates, shown as 0:00).
pub fn clock(s: f64) -> String {
    let s = s.max(0.0).floor() as i64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Parses `m:ss`, `mm:ss` or plain seconds. `None` for anything else.
pub fn parse_clock(s: &str) -> Option<f64> {
    let s = s.trim();
    match s.split_once(':') {
        Some((m, sec)) => {
            let m: u32 = m.parse().ok()?;
            let sec: u32 = sec.parse().ok()?;
            (sec < 60).then_some((m * 60 + sec) as f64)
        }
        None => s.parse::<u32>().ok().map(f64::from),
    }
}

/// Plain words for a camp reason code of the API.
pub fn camp_reason_label(code: &str) -> &'static str {
    match code {
        "traded_for_objective" => "Traded for an objective",
        "before_objective" => "Before an objective",
        "after_won_fight" => "After a won fight",
        _ => "Quiet moment",
    }
}

/// Camp reasons in display order, with the count per team.
pub fn camp_table(camps: &[Camp]) -> Vec<(&'static str, [u32; 2])> {
    [
        "traded_for_objective",
        "before_objective",
        "after_won_fight",
        "quiet",
    ]
    .iter()
    .map(|code| {
        let mut c = [0, 0];
        for x in camps.iter().filter(|x| x.reason == *code) {
            c[x.team as usize % 2] += 1;
        }
        (camp_reason_label(code), c)
    })
    .collect()
}

/// Big numbers as `12.3k`, small ones as is.
pub fn short_num(v: f64) -> String {
    if v.abs() >= 1000.0 {
        format!("{:.1}k", v / 1000.0)
    } else {
        format!("{}", v.round() as i64)
    }
}

/// First stat whose top row is `match_id` (`rows` is sorted best-first by the server): the
/// `/record-flash` source's "did the newest game set a record" check. `records?mine=true` already
/// restricts every row to the operator, so no extra ownership check is needed here.
#[must_use]
pub fn new_personal_record(stats: &[RecordStat], match_id: i64) -> Option<(String, f64)> {
    stats.iter().find_map(|s| {
        let top = s.rows.first()?;
        (top.match_id == match_id).then(|| (s.label.clone(), top.value))
    })
}

/// Query string of `/api/records`.
pub fn records_query(
    mode: Option<i64>,
    map: Option<&str>,
    hero: Option<&str>,
    mine: bool,
) -> String {
    let mut q = Vec::new();
    if let Some(m) = mode {
        q.push(format!("mode={m}"));
    }
    if let Some(m) = map.filter(|m| !m.is_empty()) {
        q.push(format!("map={}", crate::route::uri_component(m)));
    }
    if let Some(h) = hero.filter(|h| !h.is_empty()) {
        q.push(format!("hero={}", crate::route::uri_component(h)));
    }
    if mine {
        q.push("mine=true".into());
    }
    q.join("&")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn clock_round_trips() {
        assert_eq!(clock(75.9), "1:15");
        assert_eq!(clock(-30.0), "0:00");
        assert_eq!(parse_clock("1:15"), Some(75.0));
        assert_eq!(parse_clock(" 12:05 "), Some(725.0));
        assert_eq!(parse_clock("90"), Some(90.0));
        assert_eq!(parse_clock("1:75"), None);
        assert_eq!(parse_clock("abc"), None);
    }

    #[test]
    fn camp_table_counts_per_reason_and_team() {
        let c = |team, reason: &str| Camp {
            time: 0.0,
            team,
            kind: "Siege Camp".into(),
            reason: reason.into(),
        };
        let t = camp_table(&[c(0, "quiet"), c(1, "quiet"), c(1, "before_objective")]);
        assert_eq!(t[1], ("Before an objective", [0, 1]));
        assert_eq!(t[3], ("Quiet moment", [1, 1]));
    }

    #[test]
    fn small_helpers() {
        assert_eq!(pct(1, 3), 33);
        assert_eq!(pct(1, 0), 0);
        assert_eq!(short_num(12345.0), "12.3k");
        assert_eq!(short_num(999.4), "999");
        assert_eq!(
            records_query(Some(50091), Some("Sky Temple"), None, true),
            "mode=50091&map=Sky%20Temple&mine=true"
        );
        assert_eq!(records_query(None, None, None, false), "");
    }

    #[test]
    fn new_personal_record_finds_the_first_matching_stat() {
        let row = |match_id, value| RecordRow {
            value,
            toon: "t".into(),
            name: None,
            hero: None,
            map: None,
            played_at: None,
            match_id,
            mine: true,
        };
        let stats = vec![
            RecordStat {
                key: "kills".into(),
                label: "Most kills".into(),
                rows: vec![row(10, 12.0)],
            },
            RecordStat {
                key: "takedowns".into(),
                label: "Most takedowns".into(),
                rows: vec![row(11, 20.0)],
            },
        ];
        assert_eq!(
            new_personal_record(&stats, 11),
            Some(("Most takedowns".to_string(), 20.0))
        );
        assert_eq!(new_personal_record(&stats, 999), None);
        assert_eq!(
            new_personal_record(
                &[RecordStat {
                    key: "k".into(),
                    label: "L".into(),
                    rows: vec![]
                }],
                1
            ),
            None
        );
    }

    #[test]
    fn parses_the_match_insights_answer() {
        let j = r#"{"fights":[{"start":1.0,"end":2.0,"lost":[2,1],"winner":1,"deaths":[],
            "structures_after":[0,1],"camps_after":[0,0],"objectives_after":[0,0]}],
            "camps":[{"time":3.0,"team":0,"type":"Siege Camp","reason":"quiet"}],
            "isolated_deaths":[],"window":null,
            "teamfight_totals":[{"toon":"t","hero_damage":10.0,"damage_taken":null,"healing":null}]}"#;
        let m: MatchInsights = serde_json::from_str(j).unwrap();
        assert_eq!(m.fights[0].winner, Some(1));
        assert_eq!(m.camps[0].kind, "Siege Camp");
    }
}
