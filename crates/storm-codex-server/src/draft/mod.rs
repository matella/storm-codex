//! Moteur de draft HotS — pur (aucune I/O). Un format = une liste ordonnée d'étapes (rôle d'ordre
//! + action) ; la machine d'état « marche » dessus. Le côté visuel (blue/red) est découplé de
//! l'ordre : `first_pick` (un côté) dit qui joue le rôle First. Sérialisable (persistance + WS).
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

mod formats;
pub use formats::steps_for;

pub const SCHEMA_VERSION: i32 = 1;

/// Côté visuel (couleur/position) — INDÉPENDANT de l'ordre de draft.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Blue,
    Red,
}
impl Side {
    pub fn other(self) -> Side {
        match self {
            Side::Blue => Side::Red,
            Side::Red => Side::Blue,
        }
    }
}

/// Rôle d'ordre — qui draft en premier. `first_pick: Side` mappe First→un côté.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Order {
    First,
    Second,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Ban,
    Pick,
}

/// Une étape est définie en rôle d'ordre (pas en couleur) : c'est ce qui décorrèle first-pick et side.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Step {
    pub order: Order,
    pub action: Action,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Standard,
    Normal,
    Fearless,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DraftError {
    Unavailable,
    Complete,
    NothingToUndo,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TeamInfo {
    pub name: String,
    pub players: [String; 5],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DraftState {
    pub schema_version: i32,
    pub format: Format,
    /// Quel CÔTÉ joue le rôle First — réglage indépendant du nommage blue/red.
    pub first_pick: Side,
    pub map: String,
    pub blue: TeamInfo,
    pub red: TeamInfo,
    /// [blue, red].
    pub score: [u8; 2],
    pub bo: u8,
    /// En rôle d'ordre (First/Second), indépendant du side.
    pub steps: Vec<Step>,
    pub cursor: usize,
    /// Parallèle à `steps` : héros assigné à l'étape i (None si pas encore joué).
    pub assignments: Vec<Option<String>>,
    pub manual_unavailable: BTreeSet<String>,
    /// Fearless : héros pické aux parties précédentes de la série.
    pub series_bans: BTreeSet<String>,
}

impl DraftState {
    pub fn new(format: Format, first_pick: Side, map: String) -> Self {
        let steps = steps_for(format);
        let n = steps.len();
        Self {
            schema_version: SCHEMA_VERSION,
            format,
            first_pick,
            map,
            blue: TeamInfo { name: "Blue".into(), players: Default::default() },
            red: TeamInfo { name: "Red".into(), players: Default::default() },
            score: [0, 0],
            bo: 5,
            steps,
            cursor: 0,
            assignments: vec![None; n],
            manual_unavailable: BTreeSet::new(),
            series_bans: BTreeSet::new(),
        }
    }

    /// Résout un rôle d'ordre en côté visuel selon `first_pick` (le découplage).
    pub fn side_of(&self, order: Order) -> Side {
        if order == Order::First {
            self.first_pick
        } else {
            self.first_pick.other()
        }
    }
    pub fn current_step(&self) -> Option<Step> {
        self.steps.get(self.cursor).copied()
    }
    pub fn active_side(&self) -> Option<Side> {
        self.current_step().map(|s| self.side_of(s.order))
    }
    pub fn is_complete(&self) -> bool {
        self.cursor >= self.steps.len()
    }

    fn is_used(&self, hero: &str) -> bool {
        self.assignments.iter().flatten().any(|h| h == hero)
    }
    pub fn is_unavailable(&self, hero: &str) -> bool {
        self.manual_unavailable.contains(hero) || self.series_bans.contains(hero) || self.is_used(hero)
    }

    pub fn apply(&mut self, hero: &str) -> Result<(), DraftError> {
        if self.is_complete() {
            return Err(DraftError::Complete);
        }
        if self.is_unavailable(hero) {
            return Err(DraftError::Unavailable);
        }
        self.assignments[self.cursor] = Some(hero.to_string());
        self.cursor += 1;
        Ok(())
    }
    pub fn undo(&mut self) -> Result<(), DraftError> {
        if self.cursor == 0 {
            return Err(DraftError::NothingToUndo);
        }
        self.cursor -= 1;
        self.assignments[self.cursor] = None;
        Ok(())
    }
    pub fn reset(&mut self) {
        self.cursor = 0;
        self.assignments = vec![None; self.steps.len()];
    }
    pub fn set_unavailable(&mut self, hero: &str, value: bool) {
        if value {
            self.manual_unavailable.insert(hero.to_string());
        } else {
            self.manual_unavailable.remove(hero);
        }
    }
    pub fn seed_series(&mut self, heroes: &[String]) {
        self.series_bans = heroes.iter().cloned().collect();
    }
    /// Héros pické dans la partie courante (pour alimenter l'historique de série fearless).
    pub fn picked_heroes(&self) -> Vec<String> {
        self.steps
            .iter()
            .zip(&self.assignments)
            .filter(|(s, _)| s.action == Action::Pick)
            .filter_map(|(_, a)| a.clone())
            .collect()
    }
    /// Longueur du bloc d'étapes consécutives du même rôle d'ordre (= même côté) depuis le curseur,
    /// pour le timer **par phase** (le compte à rebours couvre tout le bloc, pas chaque pick).
    pub fn current_phase_len(&self) -> usize {
        let Some(step) = self.current_step() else {
            return 0;
        };
        self.steps[self.cursor..]
            .iter()
            .take_while(|s| s.order == step.order)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_standard() -> DraftState {
        DraftState::new(Format::Standard, Side::Blue, "Sky Temple".into())
    }

    #[test]
    fn walks_full_standard_then_completes() {
        let mut d = new_standard();
        for i in 0..16 {
            d.apply(&format!("hero{i}")).unwrap();
        }
        assert!(d.is_complete());
        assert_eq!(d.apply("late"), Err(DraftError::Complete));
    }

    #[test]
    fn rejects_already_used_hero() {
        let mut d = new_standard();
        d.apply("Muradin").unwrap();
        assert_eq!(d.apply("Muradin"), Err(DraftError::Unavailable));
    }

    #[test]
    fn undo_frees_last_hero() {
        let mut d = new_standard();
        d.apply("Muradin").unwrap();
        d.undo().unwrap();
        assert_eq!(d.cursor, 0);
        d.apply("Muradin").unwrap();
        assert_eq!(d.undo(), Ok(()));
        d.undo().unwrap_err();
        assert_eq!(d.undo(), Err(DraftError::NothingToUndo));
    }

    #[test]
    fn reset_clears_picks_keeps_config() {
        let mut d = new_standard();
        d.blue.name = "Germany".into();
        d.apply("Muradin").unwrap();
        d.reset();
        assert_eq!(d.cursor, 0);
        assert!(d.assignments.iter().all(|a| a.is_none()));
        assert_eq!(d.blue.name, "Germany");
    }

    #[test]
    fn fearless_seed_blocks_series_heroes() {
        let mut d = DraftState::new(Format::Fearless, Side::Blue, "Sky Temple".into());
        d.seed_series(&["Jaina".into()]);
        assert_eq!(d.apply("Jaina"), Err(DraftError::Unavailable));
    }

    #[test]
    fn manual_override_toggles_availability() {
        let mut d = new_standard();
        d.set_unavailable("Abathur", true);
        assert_eq!(d.apply("Abathur"), Err(DraftError::Unavailable));
        d.set_unavailable("Abathur", false);
        d.apply("Abathur").unwrap();
    }

    #[test]
    fn first_pick_is_independent_of_side() {
        // first_pick = Red : le rôle First est joué par le CÔTÉ rouge.
        let d = DraftState::new(Format::Standard, Side::Red, "Sky Temple".into());
        assert_eq!(d.active_side(), Some(Side::Red));
        let d2 = DraftState::new(Format::Standard, Side::Blue, "Sky Temple".into());
        assert_eq!(d2.active_side(), Some(Side::Blue));
    }

    #[test]
    fn current_phase_len_groups_consecutive_same_order() {
        let mut d = new_standard();
        assert_eq!(d.current_phase_len(), 1); // étape 1 = ban First, isolée
        for i in 0..5 {
            d.apply(&format!("h{i}")).unwrap();
        }
        assert_eq!(d.current_phase_len(), 2); // étapes 6-7 = 2 picks Second consécutifs
    }

    #[test]
    fn picked_heroes_excludes_bans() {
        let mut d = new_standard();
        for i in 0..5 {
            d.apply(&format!("h{i}")).unwrap();
        }
        // 4 bans + 1 pick joués → un seul pické (h4).
        assert_eq!(d.picked_heroes(), vec!["h4".to_string()]);
    }
}
