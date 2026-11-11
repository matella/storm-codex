//! Hero page: wire types and the pure rules (best map, talent build steps).
//! Port of the logic of `web/src/pages/Hero.tsx`.

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct HeroMap {
    /// `null` for the games without a known map.
    pub map: Option<String>,
    pub games: i64,
    pub wins: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct HeroBuild {
    /// `TierNChoice` → talent tree id.
    pub talents: BTreeMap<String, String>,
    pub games: i64,
    pub wins: i64,
}

/// `GET /api/hero/:name`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct HeroDetail {
    pub hero: String,
    pub games: i64,
    pub wins: i64,
    pub avg_kills: Option<f64>,
    pub avg_deaths: Option<f64>,
    pub avg_takedowns: Option<f64>,
    pub by_map: Vec<HeroMap>,
    pub builds: Vec<HeroBuild>,
}

/// One talent of the reference data (`GET /api/dim/talents`, keyed by talent tree id).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct DimTalent {
    pub name: String,
    pub tier: i64,
}

/// Maps of the hero, most played first (stable for equal counts).
pub fn maps_by_games(maps: &[HeroMap]) -> Vec<HeroMap> {
    let mut out = maps.to_vec();
    out.sort_by_key(|m| std::cmp::Reverse(m.games));
    out
}

/// Best map with 5+ games: highest win rate, the most played one winning ties (input is most played first).
pub fn best_map(maps: &[HeroMap]) -> Option<&HeroMap> {
    let mut best: Option<&HeroMap> = None;
    for m in maps.iter().filter(|m| m.games >= 5) {
        let rate = |x: &HeroMap| x.wins as f64 / x.games as f64;
        if best.is_none_or(|b| rate(m) > rate(b)) {
            best = Some(m);
        }
    }
    best
}

/// `tierNum`: the first number of a `TierNChoice` key (0 without one).
fn tier_num(key: &str) -> i64 {
    let digits: String = key
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().unwrap_or(0)
}

/// Talent tree ids of a build in level order (`Tier1Choice`, `Tier2Choice`...); other keys are ignored.
pub fn build_picks(talents: &BTreeMap<String, String>) -> Vec<String> {
    let mut picks: Vec<(&String, &String)> = talents
        .iter()
        .filter(|(k, _)| {
            k.strip_prefix("Tier")
                .is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit()))
        })
        .collect();
    picks.sort_by_key(|(k, _)| tier_num(k));
    picks.into_iter().map(|(_, id)| id.clone()).collect()
}

/// `replace(/([a-z])([A-Z])/g, "$1 $2")`: a talent id shown as words when the reference data does not know it.
pub fn decamelize(id: &str) -> String {
    let mut out = String::with_capacity(id.len() + 4);
    let mut prev: Option<char> = None;
    for c in id.chars() {
        if c.is_ascii_uppercase() && prev.is_some_and(|p| p.is_ascii_lowercase()) {
            out.push(' ');
        }
        out.push(c);
        prev = Some(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(m: &str, g: i64, w: i64) -> HeroMap {
        HeroMap {
            map: Some(m.into()),
            games: g,
            wins: w,
        }
    }

    #[test]
    fn the_best_map_needs_five_games_and_ties_go_to_the_most_played() {
        let maps = maps_by_games(&[
            map("A", 4, 4),
            map("B", 10, 6),
            map("C", 5, 3),
            map("D", 20, 12),
        ]);
        assert_eq!(
            maps.iter()
                .filter_map(|m| m.map.as_deref())
                .collect::<Vec<_>>(),
            ["D", "B", "C", "A"]
        );
        // D = B = C = 60 %: the first one (most played) stays
        assert_eq!(best_map(&maps).and_then(|m| m.map.as_deref()), Some("D"));
        assert_eq!(best_map(&[map("A", 4, 4)]), None);
    }

    #[test]
    fn build_steps_follow_the_levels_and_ignore_other_keys() {
        let t: BTreeMap<String, String> = [
            ("Tier10Choice", "ten"),
            ("Tier2Choice", "two"),
            ("Tier1Choice", "one"),
            ("Name", "x"),
            ("TierX", "y"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        assert_eq!(build_picks(&t), ["one", "two", "ten"]);
        assert_eq!(decamelize("HanzoPOTG"), "Hanzo POTG");
        assert_eq!(decamelize("ArthasRime"), "Arthas Rime");
    }
}
