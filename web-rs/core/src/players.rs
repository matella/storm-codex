//! Players, Player and Synergies pages: wire types and the pure rules (search highlighting, likely alias, sorting).
//! Port of `players.ts` and the logic of `Players.tsx`, `Player.tsx`, `Synergies.tsx`.

use serde::Deserialize;

/// A piece of a name, `hit` when it is the part that matched the search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub hit: bool,
}

/// `highlightSegments`: `name` cut around the first literal, case-insensitive occurrence of `q`; original case kept,
/// no empty segment. No highlight when lowercasing changes the length (rare special characters): better none than a
/// shifted one.
pub fn highlight_segments(name: &str, q: &str) -> Vec<Segment> {
    let whole = || {
        vec![Segment {
            text: name.to_string(),
            hit: false,
        }]
    };
    let needle: Vec<char> = q.trim().to_lowercase().chars().collect();
    let chars: Vec<char> = name.chars().collect();
    let lower: Vec<char> = name.to_lowercase().chars().collect();
    if needle.is_empty() || lower.len() != chars.len() {
        return whole();
    }
    let Some(at) = lower
        .windows(needle.len())
        .position(|w| w == needle.as_slice())
    else {
        return whole();
    };
    let seg = |a: usize, b: usize, hit: bool| Segment {
        text: chars[a..b].iter().collect(),
        hit,
    };
    let mut out = Vec::new();
    if at > 0 {
        out.push(seg(0, at, false));
    }
    out.push(seg(at, at + needle.len(), true));
    if at + needle.len() < chars.len() {
        out.push(seg(at + needle.len(), chars.len(), false));
    }
    out
}

/// Up to two letters or digits of a name, upper case (`?` when there is none).
pub fn name_initials(n: &str) -> String {
    let s: String = n
        .chars()
        .filter(|c| c.is_alphanumeric())
        .take(2)
        .collect::<String>()
        .to_uppercase();
    if s.is_empty() {
        "?".into()
    } else {
        s
    }
}

/// Rounded win percentage (0 without games).
pub fn pct(w: i64, g: i64) -> i64 {
    if g == 0 {
        0
    } else {
        ((100 * w) as f64 / g as f64).round() as i64
    }
}

/// Colour of a win rate in text (`hot` / `cold` / none), as in `Synergies.tsx` and the hero pool.
pub fn wr_tone(p: i64) -> &'static str {
    crate::aggregates::tone(p)
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct PlayerHit {
    pub toon: String,
    pub name: Option<String>,
    pub matched_name: String,
    pub matched_old: bool,
    pub games: i64,
    pub wins: i64,
    pub roles: Vec<(String, i64)>,
    pub last_seen: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct PlayerSearch {
    #[serde(default)]
    pub hits: Vec<PlayerHit>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct PlayerHero {
    pub hero: String,
    pub games: i64,
    pub wins: i64,
    pub avg_kills: Option<f64>,
    pub avg_deaths: Option<f64>,
    pub avg_takedowns: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct PlayerGame {
    pub match_id: i64,
    pub hero: Option<String>,
    pub map: Option<String>,
    pub mode: Option<i64>,
    pub win: Option<bool>,
    pub kills: Option<u32>,
    pub deaths: Option<u32>,
    pub takedowns: Option<u32>,
    pub apm: Option<f64>,
    pub played_at: Option<String>,
    pub award: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct PlayerSummary {
    pub toon: String,
    pub name: Option<String>,
    pub names: Vec<String>,
    pub matches: i64,
    pub wins: i64,
    pub avg_kills: Option<f64>,
    pub avg_deaths: Option<f64>,
    pub avg_takedowns: Option<f64>,
    pub avg_apm: Option<f64>,
    pub apm_games: i64,
    pub heroes: Vec<PlayerHero>,
    pub recent: Vec<PlayerGame>,
}

/// JS number to text (`5` not `5.0`), a dash for a missing value.
pub fn num_or_dash(v: Option<f64>) -> String {
    v.map_or("—".into(), |n| n.to_string())
}

/// Empty text for a missing number (JSX prints nothing for `null`).
pub fn num_or_blank(v: Option<f64>) -> String {
    v.map(|n| n.to_string()).unwrap_or_default()
}

/// `((takedowns ?? 0) / Math.max(1, deaths ?? 0)).toFixed(1)` of the player header.
pub fn kda_ratio(takedowns: Option<f64>, deaths: Option<f64>) -> String {
    crate::session::to_fixed1(takedowns.unwrap_or(0.0) / deaths.unwrap_or(0.0).max(1.0))
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct LinkedName {
    pub toon: String,
    pub name: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Suggestion {
    pub toon: String,
    pub name: Option<String>,
    pub level: String,
}

/// What the player header needs of `GET /api/players/:toon/identity`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Identity {
    pub confirmed: Vec<LinkedName>,
    pub suggestions: Vec<Suggestion>,
}

/// `likelyAlias`: a confirmed link first, else the first high-confidence suggestion; never a low one.
pub fn likely_alias(i: &Identity) -> Option<LinkedName> {
    if let Some(c) = i.confirmed.first() {
        return Some(c.clone());
    }
    i.suggestions
        .iter()
        .find(|s| s.level == "high")
        .map(|s| LinkedName {
            toon: s.toon.clone(),
            name: s.name.clone(),
        })
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct ChatTiming {
    pub early: f64,
    pub mid: f64,
    pub late: f64,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct TopWord {
    pub word: String,
    pub count: i64,
}

/// Chat habits of an account: measures only, never the text.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct ChatHabits {
    pub games: i64,
    pub messages: i64,
    pub per_game: f64,
    pub all_share: f64,
    pub team_share: f64,
    pub timing: ChatTiming,
    pub top_words: Vec<TopWord>,
    pub enough: bool,
}

/// `Math.round(x * 100) + "%"`.
pub fn pct_share(x: f64) -> String {
    format!("{}%", (x * 100.0).round() as i64)
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Teammate {
    pub name: String,
    pub games: i64,
    pub wins: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Enemy {
    pub hero: String,
    pub games: i64,
    pub wins: i64,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Synergies {
    pub teammates: Vec<Teammate>,
    pub enemies: Vec<Enemy>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SynergySort {
    WinRate,
    Games,
}

/// Order of a `(games, wins)` list: best win rate first (ties by volume), or most games first. Stable.
pub fn sort_synergies<T: Clone>(
    rows: &[T],
    by: SynergySort,
    stat: impl Fn(&T) -> (i64, i64),
) -> Vec<T> {
    let wr = |r: &T| {
        let (g, w) = stat(r);
        if g == 0 {
            0.0
        } else {
            w as f64 / g as f64
        }
    };
    let mut out = rows.to_vec();
    match by {
        SynergySort::WinRate => out.sort_by(|a, b| {
            wr(b)
                .total_cmp(&wr(a))
                .then_with(|| stat(b).0.cmp(&stat(a).0))
        }),
        SynergySort::Games => out.sort_by_key(|r| std::cmp::Reverse(stat(r).0)),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(t: &str, hit: bool) -> Segment {
        Segment {
            text: t.into(),
            hit,
        }
    }

    #[test]
    fn the_search_hit_is_cut_out_with_its_original_case() {
        assert_eq!(
            highlight_segments("BadSté", "ADs"),
            [seg("B", false), seg("adS", true), seg("té", false)]
        );
        assert_eq!(
            highlight_segments("matella", "mat"),
            [seg("mat", true), seg("ella", false)]
        );
        assert_eq!(
            highlight_segments("matella", "ella"),
            [seg("mat", false), seg("ella", true)]
        );
        assert_eq!(
            highlight_segments("matella", "zzz"),
            [seg("matella", false)]
        );
        assert_eq!(highlight_segments("matella", "  "), [seg("matella", false)]);
        // lowercasing changes the length: no highlight rather than a shifted one
        assert_eq!(highlight_segments("İx", "x"), [seg("İx", false)]);
    }

    #[test]
    fn initials_and_percentages() {
        assert_eq!(name_initials("[ABC] zoé"), "AB");
        assert_eq!(name_initials("--"), "?");
        assert_eq!(pct(1, 3), 33);
        assert_eq!(pct(0, 0), 0);
        assert_eq!(pct_share(0.456), "46%");
        assert_eq!(kda_ratio(Some(10.0), Some(2.0)), "5.0");
        assert_eq!(kda_ratio(None, None), "0.0");
        assert_eq!(kda_ratio(Some(3.0), Some(0.0)), "3.0");
        assert_eq!(num_or_dash(Some(5.0)), "5");
        assert_eq!(num_or_dash(Some(5.25)), "5.25");
        assert_eq!(num_or_dash(None), "—");
        assert_eq!(num_or_blank(None), "");
    }

    #[test]
    fn the_alias_is_a_confirmed_link_or_a_high_suggestion() {
        let sug = |t: &str, l: &str| Suggestion {
            toon: t.into(),
            name: Some(t.into()),
            level: l.into(),
        };
        let mut i = Identity {
            confirmed: vec![],
            suggestions: vec![sug("low", "low")],
        };
        assert_eq!(likely_alias(&i), None);
        i.suggestions.push(sug("high", "high"));
        assert_eq!(likely_alias(&i).map(|a| a.toon), Some("high".into()));
        i.confirmed.push(LinkedName {
            toon: "conf".into(),
            name: None,
        });
        assert_eq!(likely_alias(&i).map(|a| a.toon), Some("conf".into()));
    }

    #[test]
    fn synergies_sort_by_rate_then_volume_or_by_volume() {
        let rows = vec![
            Teammate {
                name: "a".into(),
                games: 10,
                wins: 5,
            },
            Teammate {
                name: "b".into(),
                games: 4,
                wins: 3,
            },
            Teammate {
                name: "c".into(),
                games: 8,
                wins: 4,
            },
            Teammate {
                name: "d".into(),
                games: 12,
                wins: 3,
            },
        ];
        let stat = |t: &Teammate| (t.games, t.wins);
        let names = |v: Vec<Teammate>| v.into_iter().map(|t| t.name).collect::<Vec<_>>();
        assert_eq!(
            names(sort_synergies(&rows, SynergySort::WinRate, stat)),
            ["b", "a", "c", "d"]
        );
        assert_eq!(
            names(sort_synergies(&rows, SynergySort::Games, stat)),
            ["d", "a", "c", "b"]
        );
    }
}
