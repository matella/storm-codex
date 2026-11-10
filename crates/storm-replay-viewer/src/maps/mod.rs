// US-24: per-map logic ISOLATED here — `extract::build` must not know any map name.
// Each submodule implements one map (V1: Braxis only); the other maps known
// to lack coverage ("gap maps", US-7/22/23) return an explicit warning rather than a
// misleading silence; the rest (generic) simply returns nothing in V1.
use crate::model::Objective;
use storm_replay::Value;

mod braxis;

/// Objectives + warnings per map (US-24). Routing by map name (case-insensitive).
pub(crate) fn objectives(map_name: &str, tracker: &[Value]) -> (Vec<Objective>, Vec<String>) {
    let m = map_name.to_lowercase();
    if m.contains("braxis") {
        return braxis::waves(tracker);
    }
    if m.contains("blackheart") || m.contains("volskaya") {
        return (
            Vec::new(),
            vec![format!("objective data unavailable: {map_name}")],
        );
    }
    (Vec::new(), Vec::new()) // generic: nothing in V1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_gap_map_to_warning() {
        let (objs, warnings) = objectives("Blackheart's Bay", &[]);
        assert!(objs.is_empty());
        assert!(
            warnings.iter().any(|w| w.contains("unavailable")),
            "warnings={warnings:?}"
        );
    }

    #[test]
    fn routes_braxis_without_panicking_on_empty_tracker() {
        let (objs, warnings) = objectives("Braxis Holdout", &[]);
        assert!(objs.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn generic_map_returns_nothing() {
        let (objs, warnings) = objectives("Tomb of the Spider Queen", &[]);
        assert!(objs.is_empty());
        assert!(warnings.is_empty());
    }
}
