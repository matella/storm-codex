// US-24 : logique par carte ISOLÉE ici — `extract::build` ne doit connaître aucun nom de carte.
// Chaque sous-module implémente une carte (V1 : Braxis seulement) ; les autres cartes connues
// pour manquer de couverture ("gap maps", US-7/22/23) renvoient un warning explicite plutôt qu'un
// silence trompeur ; le reste (générique) renvoie simplement rien en V1.
use crate::model::Objective;
use storm_replay::Value;

mod braxis;

/// Objectifs + warnings par carte (US-24). Routage par nom de carte (insensible à la casse).
pub(crate) fn objectives(map_name: &str, tracker: &[Value]) -> (Vec<Objective>, Vec<String>) {
    let m = map_name.to_lowercase();
    if m.contains("braxis") {
        return braxis::waves(tracker);
    }
    if m.contains("blackheart") || m.contains("volskaya") {
        return (Vec::new(), vec![format!("objective data unavailable: {map_name}")]);
    }
    (Vec::new(), Vec::new()) // générique : rien en V1
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
