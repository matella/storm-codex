//! HotS draft engine — pure (no I/O). A format = an ordered list of steps (each step =
//! order role and action); the state machine "walks" on it. The visual side (blue/red) is
//! decoupled from the order: `first_pick` says which side plays the First role. Serialisable (persistence, WS).
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

mod formats;
pub use formats::steps_for;

pub mod api;
mod replay;
pub mod replay_api;
pub mod replay_extract;
pub mod store;
pub use replay::ReplayState;

pub const SCHEMA_VERSION: i32 = 2;

/// Visual side (colour/position) — INDEPENDENT of the draft order.
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

/// Order role — who drafts first. `first_pick: Side` maps First→a side.
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

/// A step is defined in order role (not in colour): that is what decouples first-pick and side.
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
    /// No replay game loaded/selected (or index outside the queue).
    NoGame,
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
    /// Which SIDE plays the First role — setting independent of the blue/red naming.
    pub first_pick: Side,
    pub map: String,
    pub blue: TeamInfo,
    pub red: TeamInfo,
    /// [blue, red].
    pub score: [u8; 2],
    pub bo: u8,
    /// In order role (First/Second), independent of the side.
    pub steps: Vec<Step>,
    pub cursor: usize,
    /// Parallel to `steps`: hero assigned to step i (None if not played yet).
    pub assignments: Vec<Option<String>>,
    pub manual_unavailable: BTreeSet<String>,
    /// Fearless: heroes picked in the previous games of the series (derived from `series_games`).
    pub series_bans: BTreeSet<String>,
    /// Series history (fearless): picks of each finished game.
    #[serde(default)]
    pub series_games: Vec<Vec<String>>,
    /// Draft from a replay (queue, auto playback) — empty outside this mode. See `replay.rs`.
    #[serde(default)]
    pub replay: ReplayState,
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
            blue: TeamInfo {
                name: "Blue".into(),
                players: Default::default(),
            },
            red: TeamInfo {
                name: "Red".into(),
                players: Default::default(),
            },
            score: [0, 0],
            bo: 5,
            steps,
            cursor: 0,
            assignments: vec![None; n],
            manual_unavailable: BTreeSet::new(),
            series_bans: BTreeSet::new(),
            series_games: Vec::new(),
            replay: ReplayState::default(),
        }
    }

    /// Resolves an order role into a visual side according to `first_pick` (the decoupling).
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
        self.manual_unavailable.contains(hero)
            || self.series_bans.contains(hero)
            || self.is_used(hero)
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
    /// Closes the current game: archives its picks and starts a new game whose series
    /// bans (fearless) accumulate all the picks of the previous games. Keeps the config + the score.
    pub fn start_next_game(&mut self) {
        self.series_games.push(self.picked_heroes());
        let seed: BTreeSet<String> = self.series_games.iter().flatten().cloned().collect();
        self.reset();
        self.series_bans = seed;
    }
    /// Starts over on a fresh series (clears the history + the series bans).
    pub fn new_series(&mut self) {
        self.series_games.clear();
        self.series_bans.clear();
        self.reset();
    }
    /// Aligns the engine on the format of a starting cast series: `format` + `steps` rebuilt,
    /// new game (cursor 0, empty assignments) and NEW series (history and series bans cleared).
    /// Keeps map, first_pick, manual pre-bans, teams and the replay queue (current game deselected, auto playback stopped).
    pub fn align_to_series(&mut self, format: Format) {
        self.format = format;
        self.steps = steps_for(format);
        self.new_series();
        // like `config`: the queue survives (queue, fearless, interval), but the current game is deselected
        // and auto playback stopped (its script no longer matches the rebuilt steps)
        let replay = std::mem::take(&mut self.replay);
        self.replay = ReplayState {
            current: None,
            playing: false,
            next_at_ms: None,
            ..replay
        };
    }
    /// Heroes picked in the current game (to feed the fearless series history).
    pub fn picked_heroes(&self) -> Vec<String> {
        self.steps
            .iter()
            .zip(&self.assignments)
            .filter(|(s, _)| s.action == Action::Pick)
            .filter_map(|(_, a)| a.clone())
            .collect()
    }
    /// Length of the block of consecutive steps of the same order role (= same side) from the cursor,
    /// for the **per-phase** timer (the countdown covers the whole block, not each pick).
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
#[allow(clippy::unwrap_used, clippy::expect_used)]
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
    fn align_to_series_rebuilds_steps_and_clears_series() {
        let mut d = new_standard();
        d.set_unavailable("Valla", true);
        d.apply("Muradin").unwrap();
        d.start_next_game();
        d.apply("Jaina").unwrap();
        d.blue.name = "Germany".into();
        d.align_to_series(Format::Fearless);
        assert_eq!(d.format, Format::Fearless);
        assert_eq!(d.steps, steps_for(Format::Fearless));
        assert_eq!(d.cursor, 0);
        assert_eq!(d.assignments, vec![None; d.steps.len()]);
        assert!(d.series_bans.is_empty() && d.series_games.is_empty());
        assert!(d.manual_unavailable.contains("Valla"));
        assert_eq!(d.blue.name, "Germany");
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
        // first_pick = Red: the First role is played by the red SIDE.
        let d = DraftState::new(Format::Standard, Side::Red, "Sky Temple".into());
        assert_eq!(d.active_side(), Some(Side::Red));
        let d2 = DraftState::new(Format::Standard, Side::Blue, "Sky Temple".into());
        assert_eq!(d2.active_side(), Some(Side::Blue));
    }

    #[test]
    fn current_phase_len_groups_consecutive_same_order() {
        let mut d = new_standard();
        assert_eq!(d.current_phase_len(), 1); // step 1 = First ban, isolated
        for i in 0..5 {
            d.apply(&format!("h{i}")).unwrap();
        }
        assert_eq!(d.current_phase_len(), 2); // steps 6-7 = 2 consecutive Second picks
    }

    #[test]
    fn start_next_game_accumulates_fearless_bans() {
        let mut d = DraftState::new(Format::Fearless, Side::Blue, "Sky Temple".into());
        for i in 0..16 {
            d.apply(&format!("h{i}")).unwrap();
        }
        let picks = d.picked_heroes(); // 10 picks of game 1
        d.start_next_game();
        assert_eq!(d.cursor, 0);
        assert_eq!(d.series_games.len(), 1);
        // each hero picked in game 1 is now unavailable in game 2
        for h in &picks {
            assert!(d.is_unavailable(h), "{h} should be banned (fearless)");
        }
        d.new_series();
        assert!(d.series_games.is_empty());
        assert!(!d.is_unavailable(&picks[0]));
    }

    #[test]
    fn picked_heroes_excludes_bans() {
        let mut d = new_standard();
        for i in 0..5 {
            d.apply(&format!("h{i}")).unwrap();
        }
        // 4 bans + 1 pick played → a single picked one (h4).
        assert_eq!(d.picked_heroes(), vec!["h4".to_string()]);
    }
}
