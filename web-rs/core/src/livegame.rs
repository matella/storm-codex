//! Live game feed (`GET /api/live`): wire types and the pure helpers. Port of `web/src/live.ts`.
//! Spec: docs/specs/2026-10-24-feed-live-en-partie-design.md.

use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    #[default]
    SelfOnly,
    Allies,
    All,
}

impl Scope {
    pub fn id(self) -> &'static str {
        match self {
            Scope::SelfOnly => "self",
            Scope::Allies => "allies",
            Scope::All => "all",
        }
    }
    pub fn parse(s: &str) -> Scope {
        match s {
            "allies" => Scope::Allies,
            "all" => Scope::All,
            _ => Scope::SelfOnly,
        }
    }
}

/// The visibility levels: id, label, hint.
pub const SCOPES: [(Scope, &str, &str); 3] = [
    (
        Scope::SelfOnly,
        "Me + scoreboard",
        "Your abilities, plus what the in-game scoreboard shows everyone.",
    ),
    (
        Scope::Allies,
        "+ Allies",
        "Adds your 4 allies' ability presses.",
    ),
    (
        Scope::All,
        "Everything",
        "Adds the enemy team's ability presses.",
    ),
];

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Ability {
    pub link: i64,
    pub cmd: i64,
    pub count: i64,
    pub recent: Vec<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Talent {
    pub tree_id: String,
    #[serde(rename = "loop")]
    pub at_loop: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct LivePlayer {
    pub player_id: i64,
    pub team: i64,
    pub hero: Option<String>,
    pub talents: Vec<Talent>,
    pub deaths: i64,
    pub takedowns: i64,
    /// `None`: hidden by the visibility level; `Some([])`: visible, no press yet.
    pub abilities: Option<Vec<Ability>>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct FeedItem {
    #[serde(rename = "loop")]
    pub at_loop: f64,
    pub kind: String,
    pub player: Option<i64>,
    pub team: Option<i64>,
    pub detail: Value,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Protocol {
    pub assumed_latest: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Bytes {
    pub tracker: f64,
    pub game: f64,
}

/// What the page needs of the filtered view of the game in progress.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct LiveView {
    pub session: String,
    /// `live`, `ended` or `unreadable`.
    pub status: String,
    pub reason: Option<String>,
    pub me: Option<i64>,
    /// `explicit`, `lobby` or `single_human`.
    pub me_source: Option<String>,
    pub map: Option<String>,
    pub gates_loop: Option<f64>,
    pub tracker_loop: f64,
    pub game_loop: f64,
    pub last_chunk_at: String,
    pub protocol: Protocol,
    pub bytes: Bytes,
    pub team_levels: [i64; 2],
    pub players: Vec<LivePlayer>,
    pub feed: Vec<FeedItem>,
}

pub const LOOPS_PER_SEC: f64 = 16.0;
/// Without a new chunk for that long the page warns that the feed is interrupted.
pub const STALE_AFTER_SEC: f64 = 60.0;
/// Window of the ability timeline (the `recent` window served by the server).
pub const ABILITY_WINDOW_SEC: f64 = 180.0;

/// Game clock `m:ss` counted from the opening of the gates; `-m:ss` before.
pub fn game_clock(at_loop: f64, gates_loop: Option<f64>) -> String {
    let secs = ((at_loop - gates_loop.unwrap_or(0.0)) / LOOPS_PER_SEC).trunc() as i64;
    let a = secs.abs();
    format!(
        "{}{}:{:02}",
        if secs < 0 { "-" } else { "" },
        a / 60,
        a % 60
    )
}

/// Lag of the abilities behind the tracker in game seconds (never negative); `None` without ability data.
pub fn ability_lag_sec(v: &LiveView) -> Option<i64> {
    if v.bytes.game == 0.0 {
        return None;
    }
    Some(
        ((v.tracker_loop - v.game_loop) / LOOPS_PER_SEC)
            .round_js()
            .max(0.0) as i64,
    )
}

trait RoundJs {
    fn round_js(self) -> f64;
}
impl RoundJs for f64 {
    /// `Math.round`: halves go up.
    fn round_js(self) -> f64 {
        (self + 0.5).floor()
    }
}

/// Seconds since an instant (both in epoch milliseconds), never negative.
pub fn seconds_since(then_ms: f64, now_ms: f64) -> i64 {
    ((now_ms - then_ms) / 1000.0).round_js().max(0.0) as i64
}

/// Interrupted: still `live` but no chunk for `STALE_AFTER_SEC`.
pub fn is_stale(status: &str, since_chunk_sec: i64) -> bool {
    status == "live" && since_chunk_sec as f64 > STALE_AFTER_SEC
}

/// The two teams, players sorted by id.
pub fn split_teams(players: &[LivePlayer]) -> (Vec<LivePlayer>, Vec<LivePlayer>) {
    let mut sorted = players.to_vec();
    sorted.sort_by_key(|p| p.player_id);
    let team = |t: i64| sorted.iter().filter(|p| p.team == t).cloned().collect();
    (team(0), team(1))
}

/// Label of an unnamed ability: `#491`, or `#491·2` when the command index is not 0.
pub fn ability_label(a: &Ability) -> String {
    if a.cmd != 0 {
        format!("#{}·{}", a.link, a.cmd)
    } else {
        format!("#{}", a.link)
    }
}

/// Abilities by decreasing number of presses (then by id, stable).
pub fn sort_abilities(list: &[Ability]) -> Vec<Ability> {
    let mut out = list.to_vec();
    out.sort_by_key(|a| (std::cmp::Reverse(a.count), a.link, a.cmd));
    out
}

/// Histogram of the recent presses: `bins` cells over the window, the last one being now.
pub fn recent_bins(recent: &[f64], game_loop: f64, bins: usize) -> Vec<u32> {
    let mut out = vec![0; bins];
    let span = ABILITY_WINDOW_SEC * LOOPS_PER_SEC;
    let start = game_loop - span;
    for &l in recent {
        if l < start || l > game_loop {
            continue;
        }
        let i = ((((l - start) / span) * bins as f64).floor() as usize).min(bins - 1);
        out[i] += 1;
    }
    out
}

/// Number of bins of the ability sparkline.
pub const BINS: usize = 36;

/// Current talents of a player: the last choice by tier, in tier order. A talent reset re-emits every choice: the feed
/// keeps them all, the scoreboard shows the current state only. `tier_of` is the tier of the reference data; an unknown
/// one keeps its arrival rank.
pub fn current_talents(
    talents: &[Talent],
    tier_of: impl Fn(&str) -> Option<i64>,
) -> Vec<(Talent, Option<i64>)> {
    let mut by_tier: Vec<(String, (Talent, Option<i64>))> = Vec::new();
    for (i, t) in talents.iter().enumerate() {
        let tier = tier_of(&t.tree_id);
        let key = tier.map_or(format!("#{i}"), |n| n.to_string());
        let entry = (t.clone(), tier);
        match by_tier.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = entry,
            None => by_tier.push((key, entry)),
        }
    }
    let mut out: Vec<_> = by_tier.into_iter().map(|(_, e)| e).collect();
    out.sort_by_key(|(_, tier)| tier.unwrap_or(i64::MAX));
    out
}

pub fn team_name(team: Option<i64>) -> &'static str {
    match team {
        Some(0) => "Team 1",
        Some(1) => "Team 2",
        _ => "A team",
    }
}

/// `String(d.x ?? "?")`-like text of a JSON detail field.
fn detail_text(d: &Value, key: &str, fallback: &str) -> String {
    match d.get(key) {
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(_)) | Some(Value::Null) | None => fallback.to_string(),
        Some(other) => other.to_string(),
    }
}

/// Short sentence of a feed item. `hero` names a player, `talent` a talent.
pub fn feed_text(
    item: &FeedItem,
    hero: impl Fn(i64) -> String,
    talent: impl Fn(&str) -> String,
) -> String {
    let d = &item.detail;
    match item.kind.as_str() {
        "death" => {
            let killers: Vec<i64> = d
                .get("killers")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_i64).collect())
                .unwrap_or_default();
            let who = item.player.map_or("?".to_string(), &hero);
            if killers.is_empty() {
                format!("{who} died")
            } else {
                format!(
                    "{who} killed by {}",
                    killers
                        .iter()
                        .map(|k| hero(*k))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
        "talent" => format!(
            "{} took {}",
            item.player.map_or("?".to_string(), &hero),
            talent(&detail_text(d, "tree_id", ""))
        ),
        "level" => format!(
            "{} reached level {}",
            team_name(item.team),
            detail_text(d, "level", "?")
        ),
        "camp" => format!(
            "{} captured {}",
            team_name(item.team),
            crate::herodetail::decamelize(&detail_text(d, "camp", "a camp"))
        ),
        _ => crate::herodetail::decamelize(&detail_text(d, "name", "map event")),
    }
}

const SCOPE_KEY: &str = "live.scope";
const ME_KEY: &str = "live.me";

/// Scope stored in the browser; `Me + scoreboard` by default.
pub fn parse_scope(stored: Option<&str>) -> Scope {
    stored.map_or(Scope::SelfOnly, Scope::parse)
}

/// `{session, me}` stored for the "I am" choice, one per game session.
pub fn parse_me(stored: Option<&str>, session: &str) -> Option<i64> {
    let v: Value = serde_json::from_str(stored?).ok()?;
    if v.get("session")?.as_str()? == session {
        v.get("me")?.as_f64().map(|n| n as i64)
    } else {
        None
    }
}

pub fn me_json(session: &str, me: i64) -> String {
    serde_json::json!({ "session": session, "me": me }).to_string()
}

pub fn scope_key() -> &'static str {
    SCOPE_KEY
}

pub fn me_key() -> &'static str {
    ME_KEY
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_clock_counts_from_the_gates() {
        assert_eq!(game_clock(16.0 * 75.0 + 1000.0, Some(1000.0)), "1:15");
        assert_eq!(game_clock(500.0, Some(1000.0)), "-0:31");
        assert_eq!(game_clock(32.0, None), "0:02");
    }

    #[test]
    fn staleness_and_lag() {
        let mut v = LiveView {
            tracker_loop: 1600.0,
            game_loop: 800.0,
            ..Default::default()
        };
        assert_eq!(ability_lag_sec(&v), None);
        v.bytes.game = 10.0;
        assert_eq!(ability_lag_sec(&v), Some(50));
        assert_eq!(seconds_since(1000.0, 4400.0), 3);
        assert_eq!(seconds_since(5000.0, 4400.0), 0);
        assert!(is_stale("live", 61));
        assert!(!is_stale("live", 60));
        assert!(!is_stale("ended", 600));
    }

    #[test]
    fn abilities_sort_and_bin() {
        let a = |link, cmd, count| Ability {
            link,
            cmd,
            count,
            recent: vec![],
        };
        let sorted = sort_abilities(&[a(5, 0, 1), a(3, 2, 4), a(3, 0, 4)]);
        assert_eq!(
            sorted.iter().map(ability_label).collect::<Vec<_>>(),
            ["#3", "#3·2", "#5"]
        );
        let bins = recent_bins(&[100.0, 2880.0, 2879.0, 99999.0], 2880.0, 36);
        assert_eq!(bins.len(), 36);
        assert_eq!(bins[35], 2);
        assert_eq!(bins.iter().sum::<u32>(), 3);
    }

    #[test]
    fn current_talents_keep_the_last_choice_by_tier() {
        let t = |id: &str| Talent {
            tree_id: id.into(),
            at_loop: 0.0,
        };
        let tier = |id: &str| match id {
            "a" | "a2" => Some(1),
            "b" => Some(4),
            _ => None,
        };
        let out = current_talents(&[t("b"), t("a"), t("zz"), t("a2")], tier);
        let ids: Vec<_> = out.iter().map(|(t, _)| t.tree_id.as_str()).collect();
        assert_eq!(ids, ["a2", "b", "zz"]);
    }

    #[test]
    fn feed_sentences() {
        let hero = |id: i64| format!("H{id}");
        let talent = |id: &str| format!("T:{id}");
        let it = |kind: &str, player, team, detail| FeedItem {
            kind: kind.into(),
            player,
            team,
            detail,
            ..Default::default()
        };
        assert_eq!(
            feed_text(
                &it("death", Some(1), None, json!({"killers": [2, 3]})),
                hero,
                talent
            ),
            "H1 killed by H2, H3"
        );
        assert_eq!(
            feed_text(&it("death", Some(1), None, json!({})), hero, talent),
            "H1 died"
        );
        assert_eq!(
            feed_text(
                &it("talent", Some(2), None, json!({"tree_id": "x"})),
                hero,
                talent
            ),
            "H2 took T:x"
        );
        assert_eq!(
            feed_text(
                &it("level", None, Some(0), json!({"level": 10})),
                hero,
                talent
            ),
            "Team 1 reached level 10"
        );
        assert_eq!(
            feed_text(
                &it("camp", None, Some(1), json!({"camp": "BossCamp"})),
                hero,
                talent
            ),
            "Team 2 captured Boss Camp"
        );
        assert_eq!(
            feed_text(&it("camp", None, None, json!({})), hero, talent),
            "A team captured a camp"
        );
        assert_eq!(
            feed_text(
                &it("objective", None, None, json!({"name": "DragonKnight"})),
                hero,
                talent
            ),
            "Dragon Knight"
        );
        assert_eq!(
            feed_text(&it("objective", None, None, json!({})), hero, talent),
            "map event"
        );
    }

    #[test]
    fn stored_choices() {
        assert_eq!(parse_scope(Some("all")), Scope::All);
        assert_eq!(parse_scope(Some("x")), Scope::SelfOnly);
        assert_eq!(parse_scope(None), Scope::SelfOnly);
        assert_eq!(parse_me(Some(&me_json("s1", 3)), "s1"), Some(3));
        assert_eq!(parse_me(Some(&me_json("s1", 3)), "s2"), None);
        assert_eq!(parse_me(Some("not json"), "s1"), None);
        let (t0, t1) = split_teams(&[
            LivePlayer {
                player_id: 3,
                team: 1,
                ..Default::default()
            },
            LivePlayer {
                player_id: 1,
                team: 0,
                ..Default::default()
            },
            LivePlayer {
                player_id: 2,
                team: 1,
                ..Default::default()
            },
        ]);
        assert_eq!(
            (t0.len(), t1.iter().map(|p| p.player_id).collect::<Vec<_>>()),
            (1, vec![2, 3])
        );
    }
}
