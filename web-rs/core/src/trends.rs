//! Trends page: one HotS build is one patch. Port of the logic of `web/src/pages/Trends.tsx`.

use serde::Deserialize;

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Trend {
    pub build: i64,
    pub games: i64,
    pub blue_wins: i64,
    pub avg_length: Option<f64>,
    pub first_seen: Option<String>,
    pub last_seen: Option<String>,
    pub my_games: i64,
    pub my_wins: i64,
}

/// Number of patches of the chart.
pub const CHART_PATCHES: usize = 24;

/// Chronological order for the chart (the API answers newest first): the last `CHART_PATCHES` builds.
pub fn chrono(all: &[Trend]) -> Vec<Trend> {
    let mut out = all.to_vec();
    out.sort_by_key(|t| t.build);
    let skip = out.len().saturating_sub(CHART_PATCHES);
    out.split_off(skip)
}

/// Patches with at least one game of the operator.
pub fn mine(all: &[Trend]) -> Vec<&Trend> {
    all.iter().filter(|t| t.my_games > 0).collect()
}

/// Best patch of the operator with 5+ games (the first one wins ties, in API order).
pub fn best_patch(all: &[Trend]) -> Option<&Trend> {
    let rate = |t: &Trend| t.my_wins as f64 / t.my_games as f64;
    let mut best: Option<&Trend> = None;
    for t in all.iter().filter(|t| t.my_games >= 5) {
        if best.is_none_or(|b| rate(t) > rate(b)) {
            best = Some(t);
        }
    }
    best
}

/// Height of a chart bar: `max(4, p) * 0.9` percent, 4 percent without a game.
pub fn bar_height(my_games: i64, p: i64) -> String {
    if my_games > 0 {
        format!("{}%", p.max(4) as f64 * 0.9)
    } else {
        "4%".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(build: i64, my_games: i64, my_wins: i64) -> Trend {
        Trend {
            build,
            my_games,
            my_wins,
            ..Default::default()
        }
    }

    #[test]
    fn the_chart_keeps_the_last_builds_in_order() {
        let all: Vec<Trend> = (1..=30).rev().map(|b| t(b, 0, 0)).collect();
        let c = chrono(&all);
        assert_eq!(c.len(), CHART_PATCHES);
        assert_eq!((c[0].build, c[CHART_PATCHES - 1].build), (7, 30));
        assert_eq!(chrono(&[t(2, 0, 0), t(1, 0, 0)])[0].build, 1);
    }

    #[test]
    fn the_best_patch_needs_five_games() {
        let all = [t(3, 4, 4), t(2, 10, 6), t(1, 5, 4)];
        assert_eq!(best_patch(&all).map(|b| b.build), Some(1));
        assert_eq!(best_patch(&[t(1, 4, 4)]), None);
        assert_eq!(mine(&[t(1, 0, 0), t(2, 1, 1)]).len(), 1);
    }

    #[test]
    fn bar_heights_print_like_javascript() {
        assert_eq!(bar_height(10, 70), "63%");
        assert_eq!(bar_height(10, 55), "49.5%");
        assert_eq!(bar_height(10, 0), "3.6%");
        assert_eq!(bar_height(0, 0), "4%");
    }
}
