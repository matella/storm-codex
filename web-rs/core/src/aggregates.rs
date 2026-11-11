//! Aggregate pages (Heroes, Maps, Leagues, Patch Notes): the filter shared by the aggregates, the row types and the
//! pure rules of those pages. Port of the logic in `api.ts` (`aggParams`, `aggToSearch`, `searchToAgg`) and of
//! `Heroes.tsx`, `Maps.tsx`, `Leagues.tsx`, `Patches.tsx`.

use crate::{heroes::Dim, route::query_value};
use serde::Deserialize;

/// Filter of the aggregates (`AggFilter`): mode, own games only, account, local dates `YYYY-MM-DD`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AggFilter {
    pub mode: Option<i64>,
    pub mine: bool,
    pub account: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

impl AggFilter {
    /// Anything set: the "reset" pill shows.
    pub fn is_active(&self) -> bool {
        self.mode.is_some()
            || self.mine
            || self.account.is_some()
            || self.from.is_some()
            || self.to.is_some()
    }
}

/// `encodeURIComponent`-like escaping of a query value the way `URLSearchParams.toString` does it
/// (`application/x-www-form-urlencoded`: space as `+`, everything but `A-Za-z0-9*-._` as `%XX`).
pub fn form_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'*' | b'-' | b'.' | b'_' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Query string of an aggregate request. `from_iso` / `to_iso` are the local-midnight dates already converted to ISO
/// by the browser (`to` made inclusive: the server bound is exclusive, so the caller gives the next day).
pub fn agg_query(f: &AggFilter, from_iso: Option<&str>, to_iso: Option<&str>) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(m) = f.mode {
        parts.push(format!("mode={m}"));
    }
    if f.mine {
        parts.push("mine=true".into());
    }
    if let Some(a) = f.account.as_deref().filter(|a| !a.is_empty()) {
        parts.push(format!("account={}", form_encode(a)));
    }
    if let Some(d) = from_iso {
        parts.push(format!("from={}", form_encode(d)));
    }
    if let Some(d) = to_iso {
        parts.push(format!("to={}", form_encode(d)));
    }
    parts.join("&")
}

/// Filter as raw URL parameters (dates `YYYY-MM-DD`), to carry the scope from a list to a detail page.
pub fn agg_to_search(f: &AggFilter) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(m) = f.mode {
        parts.push(format!("mode={m}"));
    }
    if f.mine {
        parts.push("mine=1".into());
    }
    if let Some(a) = f.account.as_deref().filter(|a| !a.is_empty()) {
        parts.push(format!("account={}", form_encode(a)));
    }
    if let Some(d) = f.from.as_deref().filter(|d| !d.is_empty()) {
        parts.push(format!("from={}", form_encode(d)));
    }
    if let Some(d) = f.to.as_deref().filter(|d| !d.is_empty()) {
        parts.push(format!("to={}", form_encode(d)));
    }
    parts.join("&")
}

/// Inverse of `agg_to_search`.
pub fn search_to_agg(search: &str) -> AggFilter {
    let non_empty = |k: &str| query_value(search, k).filter(|v| !v.is_empty());
    AggFilter {
        mode: non_empty("mode").and_then(|m| m.parse::<f64>().ok().map(|n| n as i64)),
        mine: query_value(search, "mine").as_deref() == Some("1"),
        account: non_empty("account"),
        from: non_empty("from"),
        to: non_empty("to"),
    }
}

/// Mode pills of the filter bar: label, mode code (`None` = all).
pub const MODES: [(&str, Option<i64>); 6] = [
    ("All", None),
    ("Storm League", Some(50091)),
    ("ARAM", Some(50101)),
    ("Custom", Some(-1)),
    ("Hero League", Some(50061)),
    ("QM", Some(50001)),
];

/// Colour class of a win rate: hot from 55 %, cold under 45 %.
pub fn tone(p: i64) -> &'static str {
    if p >= 55 {
        "hot"
    } else if p < 45 {
        "cold"
    } else {
        ""
    }
}

/// Class of the thin bar under a rate.
pub fn bar_class(tone: &str) -> &'static str {
    match tone {
        "hot" => "ds-bar win",
        "cold" => "ds-bar loss",
        _ => "ds-bar ",
    }
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct HeroStat {
    pub hero: String,
    pub games: i64,
    pub wins: i64,
}

/// Win rate of a hero tile (`Math.round(100 * (wins / games))`; the order of the operations matters at `.5`).
pub fn hero_pct(h: &HeroStat) -> i64 {
    if h.games == 0 {
        0
    } else {
        (100.0 * (h.wins as f64 / h.games as f64)).round() as i64
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeroSort {
    Games,
    WinRate,
}

/// Roles present in the list, sorted, without the multi-role entries (`a,b`).
pub fn hero_roles(rows: &[HeroStat], dim: &Dim) -> Vec<String> {
    let mut roles: Vec<String> = rows
        .iter()
        .filter_map(|h| dim.role(&h.hero))
        .filter(|r| !r.contains(','))
        .map(str::to_string)
        .collect();
    roles.sort();
    roles.dedup();
    roles
}

/// Rows of the hero wall: filtered by role, then most played or best win rate first (stable).
pub fn hero_rows(rows: &[HeroStat], dim: &Dim, role: &str, sort: HeroSort) -> Vec<HeroStat> {
    let wr = |h: &HeroStat| {
        if h.games == 0 {
            0.0
        } else {
            h.wins as f64 / h.games as f64
        }
    };
    let mut out: Vec<HeroStat> = rows
        .iter()
        .filter(|h| role.is_empty() || dim.role(&h.hero) == Some(role))
        .cloned()
        .collect();
    match sort {
        HeroSort::Games => out.sort_by_key(|h| std::cmp::Reverse(h.games)),
        HeroSort::WinRate => out.sort_by(|a, b| wr(b).total_cmp(&wr(a))),
    }
    out
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct MapStat {
    pub map: String,
    pub games: i64,
    pub blue_wins: i64,
    #[serde(default)]
    pub avg_length: Option<f64>,
    pub my_games: i64,
    pub my_wins: i64,
}

/// `Math.round((100 * wins) / games)` of the Maps page (0 without games).
pub fn map_pct(wins: i64, games: i64) -> i64 {
    if games == 0 {
        0
    } else {
        ((100 * wins) as f64 / games as f64).round() as i64
    }
}

/// The tile's rate: the operator's when he played the map, else the blue side's.
pub fn map_rate(m: &MapStat) -> (bool, i64) {
    let mine = m.my_games > 0;
    (
        mine,
        if mine {
            map_pct(m.my_wins, m.my_games)
        } else {
            map_pct(m.blue_wins, m.games)
        },
    )
}

/// Maps with an in-game minimap (ARAM maps have none): slugs of `assets/minimaps`.
const HAS_MINIMAP: [&str; 15] = [
    "alterac-pass",
    "battlefield-of-eternity",
    "blackhearts-bay",
    "braxis-holdout",
    "cursed-hollow",
    "dragon-shire",
    "garden-of-terror",
    "hanamura-temple",
    "haunted-mines",
    "infernal-shrines",
    "sky-temple",
    "tomb-of-the-spider-queen",
    "towers-of-doom",
    "volskaya-foundry",
    "warhead-junction",
];

pub fn has_map_art(map: &str) -> bool {
    !map.is_empty() && HAS_MINIMAP.contains(&crate::session::map_slug(map).as_str())
}

/// Minimap image of a map, `/images/minimaps/<slug>.jpg`.
pub fn minimap_image(map: &str) -> String {
    format!("/images/minimaps/{}.jpg", crate::session::map_slug(map))
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Team {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub roster: Option<Vec<String>>,
    #[serde(default)]
    pub league: Option<String>,
}

pub const UNASSIGNED: &str = "Unassigned";

/// Teams grouped by league (trimmed name, `Unassigned` when empty), leagues alphabetical and `Unassigned` last.
/// Team order inside a league is the server's. The alphabetical order is case-insensitive (an approximation of
/// `localeCompare`).
pub fn group_leagues(teams: &[Team]) -> Vec<(String, Vec<&Team>)> {
    let mut groups: Vec<(String, Vec<&Team>)> = Vec::new();
    for t in teams {
        let key = t
            .league
            .as_deref()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .unwrap_or(UNASSIGNED)
            .to_string();
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => v.push(t),
            None => groups.push((key, vec![t])),
        }
    }
    groups.sort_by(|a, b| match (a.0 == UNASSIGNED, b.0 == UNASSIGNED) {
        (true, _) => std::cmp::Ordering::Greater,
        (_, true) => std::cmp::Ordering::Less,
        _ => {
            a.0.to_lowercase()
                .cmp(&b.0.to_lowercase())
                .then_with(|| a.0.cmp(&b.0))
        }
    });
    groups
}

/// `1 team` / `3 teams`.
pub fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchItem {
    #[serde(default)]
    pub internal_id: String,
    #[serde(default)]
    pub patch_name: String,
    #[serde(default)]
    pub patch_type: String,
    #[serde(default)]
    pub live_date: Option<String>,
    #[serde(default)]
    pub hero_count: i64,
    #[serde(default)]
    pub map_count: i64,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct PatchList {
    #[serde(default)]
    pub items: Vec<PatchItem>,
}

/// Patch types present, in order of first appearance, without the empty one.
pub fn patch_types(items: &[PatchItem]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in items.iter().map(|p| p.patch_type.as_str()) {
        if !t.is_empty() && !out.iter().any(|o| o == t) {
            out.push(t.to_string());
        }
    }
    out
}

/// `12 heroes · 3 maps ›` of a patch row (only what is non-zero).
pub fn patch_counts(p: &PatchItem) -> String {
    let mut s = String::new();
    if p.hero_count > 0 {
        s += &format!("{} heroes", p.hero_count);
    }
    if p.hero_count > 0 && p.map_count > 0 {
        s += " · ";
    }
    if p.map_count > 0 {
        s += &format!("{} maps", p.map_count);
    }
    s + " ›"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heroes::DimHero;

    fn stat(h: &str, games: i64, wins: i64) -> HeroStat {
        HeroStat {
            hero: h.into(),
            games,
            wins,
        }
    }

    #[test]
    fn query_strings_match_urlsearchparams() {
        let f = AggFilter {
            mode: Some(50091),
            mine: true,
            account: Some("Bad Sté".into()),
            from: Some("2026-11-01".into()),
            to: Some("2026-11-09".into()),
        };
        assert_eq!(
            agg_query(&f, Some("2026-10-31T23:00:00.000Z"), Some("2026-11-09T23:00:00.000Z")),
            "mode=50091&mine=true&account=Bad+St%C3%A9&from=2026-10-31T23%3A00%3A00.000Z&to=2026-11-09T23%3A00%3A00.000Z"
        );
        assert_eq!(agg_query(&AggFilter::default(), None, None), "");
        assert_eq!(
            agg_to_search(&f),
            "mode=50091&mine=1&account=Bad+St%C3%A9&from=2026-11-01&to=2026-11-09"
        );
    }

    #[test]
    fn the_scope_survives_the_trip_through_the_url() {
        let f = AggFilter {
            mode: Some(-1),
            mine: true,
            account: Some("A&B".into()),
            from: Some("2026-11-01".into()),
            to: None,
        };
        assert_eq!(search_to_agg(&agg_to_search(&f)), f);
        assert_eq!(search_to_agg("?mode=&mine=true"), AggFilter::default());
        assert!(f.is_active() && !AggFilter::default().is_active());
    }

    #[test]
    fn rates_round_like_the_react_pages() {
        // the two pages do not compute the percentage in the same order
        assert_eq!(hero_pct(&stat("x", 0, 0)), 0);
        assert_eq!(hero_pct(&stat("x", 8, 3)), 38);
        assert_eq!(map_pct(3, 8), 38);
        assert_eq!(map_pct(0, 0), 0);
        assert_eq!(
            (tone(55), tone(54), tone(44), tone(45)),
            ("hot", "", "cold", "")
        );
        assert_eq!(bar_class("hot"), "ds-bar win");
        assert_eq!(bar_class(""), "ds-bar ");
    }

    #[test]
    fn the_hero_wall_sorts_and_filters_by_role() {
        let mut m = std::collections::HashMap::new();
        for (h, r) in [
            ("Tank1", "Tank"),
            ("Heal1", "Support"),
            ("Multi", "Tank,Bruiser"),
        ] {
            m.insert(
                h.to_string(),
                DimHero {
                    icon: None,
                    universe: None,
                    role: Some(r.into()),
                },
            );
        }
        let dim = Dim::new(m);
        let rows = vec![
            stat("Tank1", 10, 4),
            stat("Heal1", 20, 12),
            stat("Multi", 5, 5),
            stat("Nobody", 5, 1),
        ];
        assert_eq!(hero_roles(&rows, &dim), vec!["Support", "Tank"]);
        let by_games: Vec<_> = hero_rows(&rows, &dim, "", HeroSort::Games)
            .into_iter()
            .map(|h| h.hero)
            .collect();
        assert_eq!(by_games, ["Heal1", "Tank1", "Multi", "Nobody"]);
        let by_wr: Vec<_> = hero_rows(&rows, &dim, "", HeroSort::WinRate)
            .into_iter()
            .map(|h| h.hero)
            .collect();
        assert_eq!(by_wr, ["Multi", "Heal1", "Tank1", "Nobody"]);
        let tanks = hero_rows(&rows, &dim, "Tank", HeroSort::Games);
        assert_eq!(tanks.len(), 1);
    }

    #[test]
    fn a_map_tile_shows_my_rate_else_the_blue_side() {
        let mut m = MapStat {
            map: "Cursed Hollow".into(),
            games: 10,
            blue_wins: 6,
            my_games: 4,
            my_wins: 3,
            ..MapStat::default()
        };
        assert_eq!(map_rate(&m), (true, 75));
        m.my_games = 0;
        assert_eq!(map_rate(&m), (false, 60));
        assert!(has_map_art("Cursed Hollow") && has_map_art("Blackheart's Bay"));
        assert!(!has_map_art("Lost Cavern") && !has_map_art(""));
        assert_eq!(
            minimap_image("Dragon Shire"),
            "/images/minimaps/dragon-shire.jpg"
        );
    }

    #[test]
    fn leagues_are_alphabetical_with_unassigned_last() {
        let t = |id, league: Option<&str>| Team {
            id,
            name: format!("T{id}"),
            roster: None,
            league: league.map(str::to_string),
        };
        let teams = vec![
            t(1, None),
            t(2, Some("  beta ")),
            t(3, Some("Alpha")),
            t(4, Some("beta")),
            t(5, Some(" ")),
        ];
        let g = group_leagues(&teams);
        let names: Vec<_> = g.iter().map(|(k, v)| (k.as_str(), v.len())).collect();
        assert_eq!(names, [("Alpha", 1), ("beta", 2), ("Unassigned", 2)]);
        assert_eq!(plural(1, "team", "teams"), "1 team");
        assert_eq!(plural(0, "league", "leagues"), "0 leagues");
    }

    #[test]
    fn patch_rows_list_types_and_counts() {
        let p = |t: &str, h, m| PatchItem {
            patch_type: t.into(),
            hero_count: h,
            map_count: m,
            ..PatchItem::default()
        };
        let items = vec![p("PTR", 0, 0), p("Live", 4, 0), p("PTR", 1, 2), p("", 0, 0)];
        assert_eq!(patch_types(&items), ["PTR", "Live"]);
        assert_eq!(patch_counts(&items[0]), " ›");
        assert_eq!(patch_counts(&items[1]), "4 heroes ›");
        assert_eq!(patch_counts(&items[2]), "1 heroes · 2 maps ›");
    }
}
