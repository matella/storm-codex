//! Scouting reports: types of `/api/scouting/*` and the pure display helpers. Port of `web/src/scouting.ts` and of
//! `humanize` / `compactEvidence` from `web/src/reportHtml.ts`.

use serde::Deserialize;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReportStatus {
    #[default]
    Empty,
    Ready,
    Analyzed,
    Stale,
}

impl ReportStatus {
    pub fn class(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Ready => "ready",
            Self::Analyzed => "analyzed",
            Self::Stale => "stale",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Empty => "no replays",
            Self::Ready => "pack ready",
            Self::Analyzed => "draft plan ready",
            Self::Stale => "plan outdated",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Rate {
    pub k: i64,
    pub n: i64,
}

/// One line of `GET /api/scouting`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ScoutingListItem {
    pub id: i64,
    pub title: String,
    pub target_name: Option<String>,
    pub created_at: String,
    pub games: i64,
    pub record: Option<Rate>,
    pub first_date: Option<String>,
    pub last_date: Option<String>,
    pub status: ReportStatus,
}

/// `day`: the date part of an ISO timestamp, `?` when missing.
pub fn day(iso: Option<&str>) -> String {
    iso.map_or("?".to_string(), |s| s.chars().take(10).collect())
}

// ── report ──────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct HeroRow {
    pub id: String,
    pub hero: String,
    pub picks: Rate,
    pub record: Rate,
    pub by: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Count {
    pub id: String,
    pub key: String,
    pub count: Rate,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct AvgStats {
    pub kills: f64,
    pub deaths: f64,
    pub assists: f64,
    pub kill_participation_pct: f64,
    pub hero_damage_pm: f64,
    pub healing_pm: f64,
    pub time_dead_pct: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct PlayerFacts {
    pub pid: String,
    pub toon: String,
    pub name: String,
    pub core: bool,
    pub record: Rate,
    pub heroes: Vec<HeroRow>,
    pub roles: Vec<Count>,
    pub stats: AvgStats,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct MapFacts {
    pub map: String,
    pub record: Rate,
    pub wilson: Option<[f64; 2]>,
    pub picks: Vec<HeroRow>,
    pub bans: Vec<Count>,
    pub bans_against: Vec<Count>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct DraftFacts {
    pub first_pick: Rate,
    pub bans_first: Vec<Count>,
    pub bans_mid: Vec<Count>,
    pub bans_against: Vec<Count>,
    pub openers: Vec<Count>,
    pub opener_roles: Vec<Count>,
    pub last_picks: Vec<Count>,
    pub faced: Vec<HeroRow>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct LevelDiff {
    pub id: String,
    pub minute: i64,
    pub avg: f64,
    pub n: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct LengthBucket {
    pub id: String,
    pub label: String,
    pub record: Rate,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct FlowFacts {
    pub first_to_10: Rate,
    pub first_to_10_record: Rate,
    pub first_fort: Rate,
    pub first_fort_record: Rate,
    pub first_objective: Rate,
    pub first_objective_record: Rate,
    pub level_diff: Vec<LevelDiff>,
    pub length: Vec<LengthBucket>,
    pub comebacks: Rate,
    pub throws: Rate,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct GamePick {
    pub hero: String,
    pub player: String,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct GameRow {
    pub gid: i64,
    pub map: String,
    pub won: bool,
    pub first_pick: Option<bool>,
    pub picks: Vec<GamePick>,
    pub opp_picks: Vec<GamePick>,
    pub bans: Vec<String>,
    pub bans_against: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Overview {
    pub games: i64,
    pub record: Rate,
    pub excluded: i64,
    pub first_date: Option<String>,
    pub last_date: Option<String>,
    pub builds: Vec<i64>,
    pub avg_length_s: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Facts {
    pub overview: Overview,
    pub players: Vec<PlayerFacts>,
    pub maps: Vec<MapFacts>,
    pub draft: DraftFacts,
    pub flow: FlowFacts,
    pub games: Vec<GameRow>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Candidate {
    pub toon: String,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Detection {
    pub candidates: Vec<Candidate>,
    pub ambiguous: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    pub facts: Facts,
    pub detection: Detection,
    pub roster: Vec<String>,
    pub roster_auto: bool,
}

/// A fact the server resolved for a claim.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Evidence {
    pub id: String,
    pub known: bool,
    pub label: Option<String>,
    pub text: Option<String>,
    pub low: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct HeroCall {
    pub hero: String,
    pub phase: Option<String>,
    pub player: Option<String>,
    pub why: String,
    pub evidence: Vec<Evidence>,
    pub unsupported: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct MapCall {
    pub map: String,
    pub why: String,
    pub evidence: Vec<Evidence>,
    pub unsupported: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Point {
    pub point: String,
    pub evidence: Vec<Evidence>,
    pub unsupported: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct MapPlan {
    pub map: String,
    pub confidence: Option<String>,
    pub overview: String,
    pub bans: Vec<HeroCall>,
    pub picks: Vec<HeroCall>,
    pub their_picks: Vec<HeroCall>,
    pub considerations: Vec<Point>,
    pub evidence: Vec<Evidence>,
    pub unsupported: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct MapChoice {
    pub pick: Vec<MapCall>,
    pub avoid: Vec<MapCall>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct General {
    pub bans: Vec<HeroCall>,
    pub picks: Vec<HeroCall>,
    pub considerations: Vec<Point>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Analysis {
    pub summary: String,
    pub map_choice: MapChoice,
    pub maps: Vec<MapPlan>,
    pub general: General,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Tally {
    pub claims: i64,
    pub unsupported: i64,
    pub unknown_ids: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ScoutTeamPlayer {
    pub toon: String,
    pub name: String,
    pub hero: String,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ScoutGame {
    pub gid: i64,
    pub played_at: Option<String>,
    pub map: Option<String>,
    pub length_s: Option<f64>,
    pub winner: Option<i64>,
    pub target_team: Option<i64>,
    pub target_source: Option<String>,
    pub teams: [Vec<ScoutTeamPlayer>; 2],
}

/// `GET /api/scouting/{id}`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ScoutingReport {
    pub id: i64,
    pub title: String,
    pub target_name: Option<String>,
    pub anchors: Vec<String>,
    pub facts_version: i64,
    pub snapshot: Option<Snapshot>,
    pub analysis: Option<Analysis>,
    pub analysis_facts_version: Option<i64>,
    pub analysis_model: Option<String>,
    pub analysis_imported_at: Option<String>,
    pub analysis_tally: Option<Tally>,
    pub created_at: String,
    pub updated_at: String,
    pub status: ReportStatus,
    pub games: Vec<ScoutGame>,
}

/// `7/9 · 78%`; `—` without a sample.
pub fn fmt_rate(r: Option<Rate>) -> String {
    match r {
        Some(r) if r.n > 0 => format!(
            "{}/{} · {}%",
            r.k,
            r.n,
            js_round(r.k as f64 * 100.0 / r.n as f64)
        ),
        _ => "—".to_string(),
    }
}

/// `Math.round`: halves go up.
fn js_round(x: f64) -> i64 {
    (x + 0.5).floor() as i64
}

/// Percentage of a rate, `None` without a sample (`pct` of the page).
pub fn pct(r: Option<Rate>) -> Option<i64> {
    r.filter(|r| r.n > 0)
        .map(|r| js_round(r.k as f64 * 100.0 / r.n as f64))
}

/// `2/3` alone, `—` without a sample (`kn`).
pub fn kn(r: Option<Rate>) -> String {
    match r {
        Some(r) if r.n > 0 => format!("{}/{}", r.k, r.n),
        _ => "—".to_string(),
    }
}

/// `m:ss` of a duration, `?` when unknown.
pub fn fmt_len(s: Option<f64>) -> String {
    s.map_or("?".to_string(), |s| {
        format!(
            "{}:{:02}",
            (s / 60.0).floor() as i64,
            (s % 60.0).floor() as i64
        )
    })
}

/// pN → name of the target team's players.
pub fn pid_names(facts: Option<&Facts>) -> HashMap<String, String> {
    facts
        .map(|f| {
            f.players
                .iter()
                .map(|p| (p.pid.clone(), p.name.clone()))
                .collect()
        })
        .unwrap_or_default()
}

/// Anchor of a map (`#map-braxis-holdout`), identical to the HTML export's.
pub fn map_anchor(map: &str) -> String {
    let mut slug = String::new();
    let mut dash = false;
    for c in map.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
            dash = false;
        } else if !dash {
            slug.push('-');
            dash = true;
        }
    }
    format!("map-{}", slug.trim_matches('-'))
}

/// Result of a replay upload → label of the upload queue.
pub fn upload_label(
    status: Option<&str>,
    error_class: Option<&str>,
    target_team: Option<i64>,
) -> String {
    match status {
        Some("added") if target_team.is_none() => "added · team not found".to_string(),
        Some("added") => "added".to_string(),
        Some("duplicate") => "already in this report".to_string(),
        Some("parse_failed") => format!("could not read ({})", error_class.unwrap_or("error")),
        Some(other) => other.to_string(),
        None => "error".to_string(),
    }
}

/// One map of the report: the replays' facts and/or the draft plan.
#[derive(Clone, Debug, PartialEq)]
pub struct MapRow {
    pub map: String,
    pub facts: Option<MapFacts>,
    pub plan: Option<MapPlan>,
}

/// Facts and plans grouped by map (maps of both sides, played maps first in the order of the facts), matched
/// case-insensitively.
pub fn maps_with_plans(facts: Option<&Facts>, plans: &[MapPlan]) -> Vec<MapRow> {
    let key = |m: &str| m.trim().to_lowercase();
    let mut by_key: Vec<(String, MapPlan)> = Vec::new();
    for p in plans {
        let k = key(&p.map);
        if let Some(slot) = by_key.iter_mut().find(|(x, _)| *x == k) {
            slot.1 = p.clone();
        } else {
            by_key.push((k, p.clone()));
        }
    }
    let mut out = Vec::new();
    for m in facts.map(|f| f.maps.as_slice()).unwrap_or_default() {
        let k = key(&m.map);
        let plan = by_key
            .iter()
            .position(|(x, _)| *x == k)
            .map(|i| by_key.remove(i).1);
        out.push(MapRow {
            map: m.map.clone(),
            facts: Some(m.clone()),
            plan,
        });
    }
    for (_, p) in by_key {
        out.push(MapRow {
            map: p.map.clone(),
            facts: None,
            plan: Some(p),
        });
    }
    out
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Replaces every `pN` word with the player's name when known (`\bp(\d+)\b`).
fn replace_pids(text: &str, names: &HashMap<String, String>) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let boundary = i == 0 || !is_word(chars[i - 1]);
        if boundary && chars[i] == 'p' {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 1 && (j == chars.len() || !is_word(chars[j])) {
                let token: String = chars[i..j].iter().collect();
                out.push_str(names.get(&token).map_or(token.as_str(), String::as_str));
                i = j;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Removes every ` (…)` group, with the whitespace before it, up to the first `)`.
fn strip_parens(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    let mut pending = String::new();
    while let Some(c) = chars.next() {
        if c.is_whitespace() {
            pending.push(c);
        } else if c == '(' {
            for d in chars.by_ref() {
                if d == ')' {
                    break;
                }
            }
            // an unclosed group stays: JS would not match it
            pending.clear();
        } else {
            out.push_str(&pending);
            pending.clear();
            out.push(c);
        }
    }
    out.push_str(&pending);
    out
}

/// Free text of the LLM: `Solo (p5)` → `Solo`, a lone `p3` → its player's name.
pub fn humanize(text: &str, names: &HashMap<String, String>) -> String {
    // `\s*\(p\d+\)` first
    let chars: Vec<char> = text.chars().collect();
    let mut stripped = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() || chars[i] == '(' {
            let mut j = i;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if j < chars.len() && chars[j] == '(' && j + 1 < chars.len() && chars[j + 1] == 'p' {
                let mut k = j + 2;
                while k < chars.len() && chars[k].is_ascii_digit() {
                    k += 1;
                }
                if k > j + 2 && k < chars.len() && chars[k] == ')' {
                    i = k + 1;
                    continue;
                }
            }
        }
        stripped.push(chars[i]);
        i += 1;
    }
    replace_pids(&stripped, names)
}

/// Evidence readable by a player: no technical parentheses, no repeat of the board's map, names for `pN`.
pub fn compact_evidence(
    e: &Evidence,
    names: &HashMap<String, String>,
    map: Option<&str>,
) -> (String, String) {
    let mut label = strip_parens(e.label.as_deref().unwrap_or_default())
        .trim()
        .to_string();
    if let Some(m) = map.filter(|m| !m.is_empty()) {
        let prefix = format!("{m} — ");
        if let Some(rest) = label.strip_prefix(&prefix) {
            label = rest.to_string();
        }
    }
    (
        replace_pids(&label, names),
        replace_pids(e.text.as_deref().unwrap_or_default(), names),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> HashMap<String, String> {
        HashMap::from([("p3".to_string(), "Solo".to_string())])
    }

    #[test]
    fn day_cuts_the_iso_date_and_defaults_to_a_question_mark() {
        assert_eq!(day(Some("2026-11-02T10:00:00Z")), "2026-11-02");
        assert_eq!(day(None), "?");
    }

    #[test]
    fn list_items_decode_with_a_lowercase_status() {
        let v: ScoutingListItem = serde_json::from_str(
            r#"{"id":3,"title":"W4","target_name":null,"created_at":"2026-11-02T10:00:00Z","games":2,"record":{"k":1,"n":2},"first_date":null,"last_date":null,"status":"stale"}"#,
        )
        .unwrap();
        assert_eq!(v.status.label(), "plan outdated");
        assert_eq!(v.record, Some(Rate { k: 1, n: 2 }));
    }

    #[test]
    fn rates_format_like_the_react_helpers() {
        let r = Some(Rate { k: 7, n: 9 });
        assert_eq!(fmt_rate(r), "7/9 · 78%");
        assert_eq!(fmt_rate(Some(Rate { k: 0, n: 0 })), "—");
        assert_eq!(fmt_rate(None), "—");
        assert_eq!(pct(r), Some(78));
        assert_eq!(pct(Some(Rate { k: 1, n: 0 })), None);
        assert_eq!(kn(r), "7/9");
        assert_eq!(kn(None), "—");
    }

    #[test]
    fn lengths_use_minutes_and_two_digit_seconds() {
        assert_eq!(fmt_len(Some(125.0)), "2:05");
        assert_eq!(fmt_len(None), "?");
    }

    #[test]
    fn map_anchors_slug_the_name() {
        assert_eq!(map_anchor("Braxis Holdout"), "map-braxis-holdout");
        assert_eq!(
            map_anchor("Tomb of the Spider Queen"),
            "map-tomb-of-the-spider-queen"
        );
        assert_eq!(map_anchor("  Alterac Pass!"), "map-alterac-pass");
    }

    #[test]
    fn humanize_drops_pid_parentheses_and_names_bare_pids() {
        let n = names();
        assert_eq!(humanize("Solo (p3) opens", &n), "Solo opens");
        assert_eq!(humanize("p3 and p9 play", &n), "Solo and p9 play");
        assert_eq!(humanize("sp3 stays, p3x too", &n), "sp3 stays, p3x too");
    }

    #[test]
    fn compact_evidence_removes_technical_parentheses_and_the_board_map() {
        let e = Evidence {
            label: Some("Braxis Holdout — Picks (n=3)".into()),
            text: Some("p3 won 2/3".into()),
            ..Default::default()
        };
        assert_eq!(
            compact_evidence(&e, &names(), Some("Braxis Holdout")),
            ("Picks".to_string(), "Solo won 2/3".to_string())
        );
        assert_eq!(
            compact_evidence(&e, &names(), None).0,
            "Braxis Holdout — Picks"
        );
    }

    #[test]
    fn maps_with_plans_matches_case_insensitively_and_appends_plan_only_maps() {
        let facts = Facts {
            maps: vec![MapFacts {
                map: "Cursed Hollow".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let plans = vec![
            MapPlan {
                map: "cursed hollow".into(),
                ..Default::default()
            },
            MapPlan {
                map: "Sky Temple".into(),
                ..Default::default()
            },
        ];
        let rows = maps_with_plans(Some(&facts), &plans);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].facts.is_some() && rows[0].plan.is_some());
        assert!(rows[1].facts.is_none() && rows[1].plan.is_some());
    }

    #[test]
    fn upload_labels() {
        assert_eq!(
            upload_label(Some("added"), None, None),
            "added · team not found"
        );
        assert_eq!(upload_label(Some("added"), None, Some(0)), "added");
        assert_eq!(
            upload_label(Some("parse_failed"), Some("old"), None),
            "could not read (old)"
        );
        assert_eq!(upload_label(None, None, None), "error");
    }
}
