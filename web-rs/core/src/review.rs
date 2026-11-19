//! Session review (`GET /api/sessions`, `GET /api/sessions/{id}`, spec `docs/specs/2026-11-19-session-review.md`):
//! wire types and the formatting of the Session page's "Review" section. No I/O.

use crate::session::{MatchPlayer, MatchSummary};
use serde::Deserialize;
use std::collections::BTreeMap;

/// The compared stats in display order, with their labels.
pub const STATS: [(&str, &str); 7] = [
    ("kills", "Kills"),
    ("deaths", "Deaths"),
    ("assists", "Assists"),
    ("hero_damage", "Hero damage"),
    ("siege_damage", "Siege damage"),
    ("healing", "Healing"),
    ("experience", "XP"),
];

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct SessionItem {
    pub id: i64,
    pub start: String,
    pub end: String,
    pub games: u32,
    pub wins: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct SessionList {
    pub sessions: Vec<SessionItem>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ReviewGame {
    pub match_id: i64,
    pub played_at: String,
    pub hero: Option<String>,
    pub map: Option<String>,
    pub mode: Option<i64>,
    pub win: bool,
    pub length: Option<f64>,
    pub kills: Option<f64>,
    pub deaths: Option<f64>,
    pub assists: Option<f64>,
    pub award: Option<String>,
    pub deltas: BTreeMap<String, f64>,
    /// "hero" or "overall"
    pub baseline: String,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Record {
    pub wins: u32,
    pub losses: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Streaks {
    pub best_win: u32,
    pub worst_loss: u32,
    pub current: i32,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Row {
    pub name: String,
    pub games: u32,
    pub wins: u32,
    pub base_games: u32,
    pub base_wins: u32,
    pub small_sample: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Baseline {
    pub from: String,
    pub to: String,
    pub modes: Vec<Option<i64>>,
    pub games: u32,
    pub wins: u32,
    pub averages: BTreeMap<String, f64>,
    pub small_sample: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Review {
    pub id: i64,
    pub start: String,
    pub end: String,
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
    pub prev: Option<i64>,
    pub next: Option<i64>,
}

/// `?s=<id>` of the Session page.
pub fn selected(search: &str) -> Option<i64> {
    crate::route::query_value(search, "s").and_then(|v| v.parse().ok())
}

impl ReviewGame {
    /// The game as the archive list shapes it, for `GameCard` (takedowns = kills + assists).
    pub fn card(&self) -> (MatchSummary, MatchPlayer) {
        let n = |v: Option<f64>| v.map(|v| v.max(0.0).round() as u32);
        let me = MatchPlayer {
            hero: self.hero.clone(),
            win: Some(self.win),
            kills: n(self.kills),
            deaths: n(self.deaths),
            takedowns: Some(n(self.kills).unwrap_or(0) + n(self.assists).unwrap_or(0)),
            award: self.award.clone(),
            ..Default::default()
        };
        let m = MatchSummary {
            id: self.match_id,
            map: self.map.clone(),
            mode: self.mode,
            played_at: Some(self.played_at.clone()),
            length: self.length,
            ..Default::default()
        };
        (m, me)
    }

    /// Stats better than the baseline (above it; below it for deaths), as the server ranks best/worst games.
    pub fn above(&self) -> usize {
        self.deltas
            .iter()
            .filter(|(k, v)| better(k, **v) == Some(true))
            .count()
    }
}

/// Whether a delta is good (`Some(true)`), bad, or neither (zero).
pub fn better(stat: &str, delta: f64) -> Option<bool> {
    if delta == 0.0 {
        return None;
    }
    Some((delta > 0.0) != (stat == "deaths"))
}

/// Signed delta: one decimal under 100, thousands as `k` above 1,000 (`+1.2`, `−3`, `+4.5k`).
pub fn fmt_delta(v: f64) -> String {
    let sign = if v > 0.0 {
        "+"
    } else if v < 0.0 {
        "−"
    } else {
        "±"
    };
    let a = v.abs();
    let body = if a >= 1000.0 {
        format!("{:.1}k", a / 1000.0)
    } else if a >= 100.0 {
        format!("{a:.0}")
    } else {
        crate::session::to_fixed1(a)
    };
    format!("{sign}{body}")
}

/// Mean delta per stat over the session's games (games without a delta for a stat are skipped), display order.
pub fn mean_deltas(games: &[ReviewGame]) -> Vec<(&'static str, &'static str, f64)> {
    STATS
        .iter()
        .filter_map(|(k, label)| {
            let v: Vec<f64> = games
                .iter()
                .filter_map(|g| g.deltas.get(*k).copied())
                .collect();
            (!v.is_empty()).then(|| (*k, *label, v.iter().sum::<f64>() / v.len() as f64))
        })
        .collect()
}

/// "W3", "L2", "—".
pub fn streak_label(current: i32) -> String {
    match current {
        0 => "—".into(),
        c if c > 0 => format!("W{c}"),
        c => format!("L{}", -c),
    }
}

/// A wall-clock span: "4 h 05", "45 min".
pub fn fmt_span(seconds: f64) -> String {
    let m = (seconds.max(0.0) / 60.0).round() as u64;
    if m < 60 {
        format!("{m} min")
    } else {
        format!("{} h {:02}", m / 60, m % 60)
    }
}

/// Win rate in percent, `None` without games.
pub fn rate(wins: u32, games: u32) -> Option<u32> {
    (games > 0).then(|| (f64::from(wins) * 100.0 / f64::from(games)).round() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(deltas: &[(&str, f64)]) -> ReviewGame {
        ReviewGame {
            match_id: 7,
            played_at: "2026-11-18T20:00:00Z".into(),
            hero: Some("Valla".into()),
            kills: Some(5.0),
            deaths: Some(2.0),
            assists: Some(7.0),
            win: true,
            deltas: deltas.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn decodes_the_review_body() {
        let r: Review = serde_json::from_value(serde_json::json!({
            "id": 3, "start": "a", "end": "b", "span_s": 10.0, "played_s": 8.0,
            "games": [{"match_id": 3, "played_at": "a", "hero": "Valla", "map": "M", "mode": 50091, "win": true,
                       "length": 600.0, "kills": 1.0, "deaths": 0.0, "assists": 2.0, "award": null,
                       "deltas": {"kills": 1.5}, "baseline": "hero"}],
            "record": {"wins": 1, "losses": 0}, "streaks": {"best_win": 1, "worst_loss": 0, "current": 1},
            "best": null, "worst": null, "heroes": [], "maps": [],
            "baseline": {"from": "x", "to": "a", "modes": [50091], "games": 0, "wins": 0, "averages": {},
                         "small_sample": true},
            "prev": 1, "next": null
        }))
        .unwrap();
        assert_eq!(r.games[0].deltas["kills"], 1.5);
        assert_eq!(r.baseline.modes, vec![Some(50091)]);
        assert_eq!((r.prev, r.next), (Some(1), None));
    }

    #[test]
    fn card_rebuilds_takedowns_for_the_kda_line() {
        let (m, me) = game(&[]).card();
        assert_eq!(m.id, 7);
        assert_eq!(crate::dashboard::kda(&me), (5, 2, 7));
    }

    #[test]
    fn fewer_deaths_is_better() {
        assert_eq!(better("kills", 1.0), Some(true));
        assert_eq!(better("deaths", 1.0), Some(false));
        assert_eq!(better("deaths", -0.5), Some(true));
        assert_eq!(better("healing", 0.0), None);
        let g = game(&[
            ("kills", 1.0),
            ("deaths", -1.0),
            ("healing", 0.0),
            ("assists", -2.0),
        ]);
        assert_eq!(g.above(), 2);
    }

    #[test]
    fn formats_deltas_and_streaks() {
        assert_eq!(fmt_delta(1.25), "+1.3");
        assert_eq!(fmt_delta(-3.0), "−3.0");
        assert_eq!(fmt_delta(4512.0), "+4.5k");
        assert_eq!(fmt_delta(-250.4), "−250");
        assert_eq!(fmt_delta(0.0), "±0.0");
        assert_eq!(streak_label(3), "W3");
        assert_eq!(streak_label(-2), "L2");
        assert_eq!(streak_label(0), "—");
        assert_eq!(rate(1, 3), Some(33));
        assert_eq!(fmt_span(17081.0), "4 h 45");
        assert_eq!(fmt_span(2700.0), "45 min");
        assert_eq!(fmt_span(3600.0), "1 h 00");
        assert_eq!(rate(0, 0), None);
    }

    #[test]
    fn means_deltas_per_stat_in_display_order() {
        let games = [
            game(&[("kills", 2.0), ("deaths", 1.0)]),
            game(&[("kills", 4.0)]),
        ];
        let m = mean_deltas(&games);
        assert_eq!(m[0], ("kills", "Kills", 3.0));
        assert_eq!(m[1], ("deaths", "Deaths", 1.0));
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn reads_the_selected_session() {
        assert_eq!(selected("?s=42"), Some(42));
        assert_eq!(selected("?s=x"), None);
        assert_eq!(selected(""), None);
    }
}
