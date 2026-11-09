//! Draft from a replay: queue of loaded games + per-phase reveal. Pure (no I/O);
//! the decoding of a `.StormReplay` and the routes live elsewhere. See `docs/specs/2026-10-25-draft-replay-design.md`.
use crate::draft::{Action, DraftError, DraftState, Format, Side};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Default auto playback interval: 30 s per phase (operator decision).
pub const DEFAULT_INTERVAL_MS: u32 = 30_000;

/// Complete draft of a game, prepared but hidden.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadedGame {
    pub source: String,
    pub map: String,
    pub first_pick: Side,
    /// Hero of step i (Standard order, 16 steps); `None` = empty step (missing ban…).
    pub script: Vec<Option<String>>,
    /// Nickname of the player of step i (picks only).
    pub pick_players: Vec<Option<String>>,
    /// Nicknames ordered by the pick ordinal of their side (step order).
    pub blue_players: [String; 5],
    pub red_players: [String; 5],
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayState {
    pub queue: Vec<LoadedGame>,
    pub current: Option<usize>,
    pub fearless: bool,
    pub playing: bool,
    pub interval_ms: u32,
    /// Timestamp (ms epoch) of the next auto playback tick, for the countdown.
    pub next_at_ms: Option<i64>,
}

impl Default for ReplayState {
    fn default() -> Self {
        Self {
            queue: Vec::new(),
            current: None,
            fearless: false,
            playing: false,
            interval_ms: DEFAULT_INTERVAL_MS,
            next_at_ms: None,
        }
    }
}

/// `[start, end)` bounds of the phases: blocks of consecutive steps of the same order role.
fn phase_bounds(d: &DraftState) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (i, s) in d.steps.iter().enumerate() {
        match out.last_mut() {
            Some((start, end)) if d.steps[*start].order == s.order => *end = i + 1,
            _ => out.push((i, i + 1)),
        }
    }
    out
}

impl DraftState {
    fn current_game(&self) -> Option<&LoadedGame> {
        self.replay.current.and_then(|i| self.replay.queue.get(i))
    }

    /// Reveals the whole current phase (the 2 picks of a double phase appear together).
    /// Writes the script as is, without a uniqueness check: a Custom replay can duplicate a hero.
    pub fn reveal_phase(&mut self) -> Result<(), DraftError> {
        let script = self
            .current_game()
            .ok_or(DraftError::NoGame)?
            .script
            .clone();
        if self.is_complete() {
            return Err(DraftError::Complete);
        }
        for _ in 0..self.current_phase_len() {
            self.assignments[self.cursor] = script.get(self.cursor).cloned().flatten();
            self.cursor += 1;
        }
        Ok(())
    }

    /// Undoes the last revealed phase (or the current phase if it has started).
    pub fn back_phase(&mut self) -> Result<(), DraftError> {
        if self.cursor == 0 {
            return Err(DraftError::NothingToUndo);
        }
        let start = phase_bounds(self)
            .into_iter()
            .find(|&(s, e)| s < self.cursor && self.cursor <= e)
            .map_or(0, |(s, _)| s);
        for a in &mut self.assignments[start..self.cursor] {
            *a = None;
        }
        self.cursor = start;
        Ok(())
    }

    /// Prepares game `index` of the queue: empty draft at step 0, teams/players/map/first-pick
    /// of the replay; in fearless, the picks of the previous games of the queue are unavailable.
    /// Keeps score, Bo, team names, manual pre-bans and the queue.
    pub fn select_game(&mut self, index: usize) -> Result<(), DraftError> {
        self.select_game_with(index, None)
    }

    /// Like `select_game`, with the format imposed by an active cast series (`Some`): it
    /// wins over `replay.fearless` (M5 — the replay queue does not bypass the series
    /// format). Fearless → Fearless + the queue's series bans; Standard/Normal → Standard (the
    /// replay scripts follow the 16-step Standard order, bans included).
    pub fn select_game_with(
        &mut self,
        index: usize,
        series_format: Option<Format>,
    ) -> Result<(), DraftError> {
        let g = self
            .replay
            .queue
            .get(index)
            .ok_or(DraftError::NoGame)?
            .clone();
        let fearless = series_format.map_or(self.replay.fearless, |f| f == Format::Fearless);
        let mut d = DraftState::new(
            if fearless {
                Format::Fearless
            } else {
                Format::Standard
            },
            g.first_pick,
            g.map,
        );
        d.blue = crate::draft::TeamInfo {
            name: self.blue.name.clone(),
            players: g.blue_players,
        };
        d.red = crate::draft::TeamInfo {
            name: self.red.name.clone(),
            players: g.red_players,
        };
        d.score = self.score;
        d.bo = self.bo;
        d.manual_unavailable = self.manual_unavailable.clone();
        d.replay = ReplayState {
            current: Some(index),
            playing: false,
            next_at_ms: None,
            ..self.replay.clone()
        };
        if fearless {
            d.series_bans = picks_of(&self.replay.queue[..index]);
        }
        *self = d;
        Ok(())
    }

    /// Adds a game; the first one added is selected automatically.
    pub fn add_game(&mut self, g: LoadedGame) {
        self.add_game_with(g, None);
    }

    /// `add_game`; the automatic selection follows the format of the active series (see `select_game_with`).
    pub fn add_game_with(&mut self, g: LoadedGame, series_format: Option<Format>) {
        self.replay.queue.push(g);
        if self.replay.current.is_none() {
            let last = self.replay.queue.len() - 1;
            let _ = self.select_game_with(last, series_format);
        }
    }

    pub fn remove_game(&mut self, index: usize) -> Result<(), DraftError> {
        if index >= self.replay.queue.len() {
            return Err(DraftError::NoGame);
        }
        self.replay.queue.remove(index);
        match self.replay.current {
            Some(c) if c == index => {
                self.replay.current = None;
                self.replay.playing = false;
                self.replay.next_at_ms = None;
                self.reset();
                self.series_bans.clear();
            }
            Some(c) if c > index => self.replay.current = Some(c - 1),
            _ => {}
        }
        Ok(())
    }

    pub fn move_game(&mut self, from: usize, to: usize) -> Result<(), DraftError> {
        let n = self.replay.queue.len();
        if from >= n || to >= n {
            return Err(DraftError::NoGame);
        }
        let cur_hero = self.replay.current;
        let g = self.replay.queue.remove(from);
        self.replay.queue.insert(to, g);
        if let Some(c) = cur_hero {
            // The current game follows its move; the other indices shift.
            self.replay.current = Some(if c == from {
                to
            } else if from < c && c <= to {
                c - 1
            } else if to <= c && c < from {
                c + 1
            } else {
                c
            });
        }
        Ok(())
    }

    /// Toggles fearless and re-prepares the current game (its series bans change).
    pub fn set_fearless(&mut self, value: bool) {
        self.set_fearless_with(value, None);
    }

    /// `set_fearless`; the re-preparation follows the format of the active series (see `select_game_with`):
    /// the flag is remembered but does not change the format as long as the series imposes it.
    pub fn set_fearless_with(&mut self, value: bool, series_format: Option<Format>) {
        self.replay.fearless = value;
        if let Some(c) = self.replay.current {
            let _ = self.select_game_with(c, series_format);
        }
    }

    /// After a server restart no timer runs: auto playback restarts paused
    /// (the cursor and the queue are kept).
    pub fn after_restart(&mut self) {
        self.replay.playing = false;
        self.replay.next_at_ms = None;
    }

    /// Public view (`GET /api/draft`, overlay): without the script, and without the nickname of an unrevealed
    /// pick in replay mode — the overlay spoils nothing.
    pub fn public(&self) -> DraftState {
        let mut p = self.clone();
        for g in &mut p.replay.queue {
            g.script.clear();
            g.pick_players.clear();
            g.blue_players = Default::default();
            g.red_players = Default::default();
        }
        if p.replay.current.is_some() {
            let (mut b, mut r) = (0usize, 0usize);
            for (i, s) in self
                .steps
                .iter()
                .enumerate()
                .filter(|(_, s)| s.action == Action::Pick)
            {
                let (team, k) = match self.side_of(s.order) {
                    Side::Blue => (&mut p.blue, &mut b),
                    Side::Red => (&mut p.red, &mut r),
                };
                if self.assignments[i].is_none() {
                    team.players[*k] = String::new();
                }
                *k += 1;
            }
        }
        p
    }
}

fn picks_of(games: &[LoadedGame]) -> BTreeSet<String> {
    games
        .iter()
        .flat_map(|g| {
            g.script.iter().enumerate().filter_map(|(i, h)| {
                // pick steps of the Standard format (bans are not "played" heroes)
                crate::draft::steps_for(Format::Standard)
                    .get(i)
                    .filter(|s| s.action == Action::Pick)
                    .and(h.clone())
            })
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn game(first: Side, tag: &str) -> LoadedGame {
        LoadedGame {
            source: format!("{tag}.StormReplay"),
            map: format!("Map {tag}"),
            first_pick: first,
            script: (0..16).map(|i| Some(format!("{tag}h{i}"))).collect(),
            pick_players: (0..16).map(|i| Some(format!("{tag}p{i}"))).collect(),
            blue_players: std::array::from_fn(|i| format!("{tag}b{i}")),
            red_players: std::array::from_fn(|i| format!("{tag}r{i}")),
        }
    }

    fn loaded() -> DraftState {
        let mut d = DraftState::new(Format::Standard, Side::Blue, "x".into());
        d.add_game(game(Side::Blue, "a"));
        d
    }

    #[test]
    fn reveal_phase_reveals_whole_two_pick_phase() {
        let mut d = loaded();
        for expected in [1, 2, 3, 4, 5] {
            d.reveal_phase().unwrap();
            assert_eq!(d.cursor, expected);
        }
        d.reveal_phase().unwrap();
        assert_eq!(d.cursor, 7);
        assert!(d.assignments[5].is_some() && d.assignments[6].is_some());
    }

    #[test]
    fn twelve_reveals_complete_standard() {
        let mut d = loaded();
        for _ in 0..12 {
            d.reveal_phase().unwrap();
        }
        assert!(d.is_complete());
        assert_eq!(d.reveal_phase(), Err(DraftError::Complete));
    }

    #[test]
    fn back_phase_undoes_whole_phase() {
        let mut d = loaded();
        for _ in 0..6 {
            d.reveal_phase().unwrap();
        }
        d.back_phase().unwrap();
        assert_eq!(d.cursor, 5);
        assert!(d.assignments[5].is_none() && d.assignments[6].is_none());
        assert!(d.assignments[4].is_some());
    }

    #[test]
    fn back_phase_at_start_errors() {
        let mut d = loaded();
        assert_eq!(d.back_phase(), Err(DraftError::NothingToUndo));
    }

    #[test]
    fn reveal_without_game_errors() {
        let mut d = DraftState::new(Format::Standard, Side::Blue, "x".into());
        assert_eq!(d.reveal_phase(), Err(DraftError::NoGame));
    }

    #[test]
    fn none_step_is_skipped() {
        let mut d = DraftState::new(Format::Standard, Side::Blue, "x".into());
        let mut g = game(Side::Blue, "a");
        g.script[2] = None;
        g.script[3] = None;
        d.add_game(g);
        for _ in 0..4 {
            d.reveal_phase().unwrap();
        }
        assert_eq!(d.cursor, 4);
        assert!(d.assignments[2].is_none() && d.assignments[3].is_none());
    }

    #[test]
    fn duplicate_hero_does_not_fail() {
        let mut d = DraftState::new(Format::Standard, Side::Blue, "x".into());
        let mut g = game(Side::Blue, "a");
        g.script[5] = g.script[0].clone();
        d.add_game(g);
        for _ in 0..6 {
            d.reveal_phase().unwrap();
        }
        assert_eq!(d.assignments[5], d.assignments[0]);
    }

    #[test]
    fn select_game_fearless_seeds_series_bans() {
        let mut d = loaded();
        d.add_game(game(Side::Red, "b"));
        d.set_fearless(true);
        d.select_game(1).unwrap();
        assert_eq!(d.format, Format::Fearless);
        assert_eq!(d.cursor, 0);
        assert_eq!(d.first_pick, Side::Red);
        assert_eq!(d.map, "Map b");
        assert_eq!(d.blue.players[0], "bb0");
        // picks (steps 4,5,6,7,8,11,12,13,14,15) of game "a", not its bans
        let want: BTreeSet<String> = [4, 5, 6, 7, 8, 11, 12, 13, 14, 15]
            .iter()
            .map(|i| format!("ah{i}"))
            .collect();
        assert_eq!(d.series_bans, want);
    }

    /// M5: an active cast series imposes its format on the replay queue, in both directions.
    #[test]
    fn select_game_with_series_format_overrides_replay_fearless() {
        let mut d = loaded();
        d.add_game(game(Side::Red, "b"));
        assert!(!d.replay.fearless);
        d.select_game_with(1, Some(Format::Fearless)).unwrap();
        assert_eq!(d.format, Format::Fearless);
        assert!(d.series_bans.contains("ah4"));
        let mut d = loaded();
        d.add_game(game(Side::Red, "b"));
        d.replay.fearless = true;
        d.select_game_with(1, Some(Format::Standard)).unwrap();
        assert_eq!(d.format, Format::Standard);
        assert!(d.series_bans.is_empty());
        // Normal (without bans) → Standard: the replay scripts have 16 steps
        d.select_game_with(1, Some(Format::Normal)).unwrap();
        assert_eq!((d.format, d.steps.len()), (Format::Standard, 16));
        // set_fearless / add_game also follow the series
        d.set_fearless_with(false, Some(Format::Fearless));
        assert_eq!(d.format, Format::Fearless);
        let mut e = DraftState::new(Format::Standard, Side::Blue, "x".into());
        e.add_game_with(game(Side::Blue, "a"), Some(Format::Fearless));
        assert_eq!(e.format, Format::Fearless);
    }

    #[test]
    fn select_game_non_fearless_has_no_series_bans() {
        let mut d = loaded();
        d.add_game(game(Side::Blue, "b"));
        d.select_game(1).unwrap();
        assert_eq!(d.format, Format::Standard);
        assert!(d.series_bans.is_empty());
    }

    #[test]
    fn public_hides_scripts() {
        let d = loaded();
        let p = d.public();
        assert!(p.replay.queue[0].script.is_empty() && p.replay.queue[0].pick_players.is_empty());
    }

    #[test]
    fn public_hides_names_of_unrevealed_picks() {
        let mut d = loaded();
        d.reveal_phase().unwrap(); // 1er ban seulement
        let p = d.public();
        assert!(p
            .blue
            .players
            .iter()
            .chain(p.red.players.iter())
            .all(String::is_empty));
        for _ in 0..4 {
            d.reveal_phase().unwrap(); // 3 more bans then the 1st Blue pick
        }
        let p = d.public();
        assert_eq!(p.blue.players[0], "ab0");
        assert!(p.blue.players[1].is_empty() && p.red.players[0].is_empty());
    }

    #[test]
    fn after_restart_pauses_but_keeps_cursor() {
        let mut d = loaded();
        d.reveal_phase().unwrap();
        d.replay.playing = true;
        d.replay.next_at_ms = Some(5);
        d.after_restart();
        assert!(!d.replay.playing && d.replay.next_at_ms.is_none());
        assert_eq!(d.cursor, 1);
    }

    #[test]
    fn remove_and_move_keep_current_consistent() {
        let mut d = loaded();
        d.add_game(game(Side::Blue, "b"));
        d.add_game(game(Side::Blue, "c"));
        d.select_game(1).unwrap();
        d.move_game(1, 2).unwrap();
        assert_eq!(d.replay.current, Some(2));
        d.remove_game(0).unwrap();
        assert_eq!(d.replay.current, Some(1));
        d.remove_game(1).unwrap();
        assert_eq!(d.replay.current, None);
        assert_eq!(d.remove_game(9), Err(DraftError::NoGame));
    }

    #[test]
    fn align_to_series_deselects_replay_game_and_stops_autoplay() {
        let mut d = loaded();
        d.add_game(game(Side::Red, "b"));
        d.replay.current = Some(1);
        d.replay.playing = true;
        d.replay.next_at_ms = Some(123);
        d.replay.fearless = true;
        d.replay.interval_ms = 7_000;
        d.align_to_series(Format::Fearless);
        assert_eq!(d.replay.current, None);
        assert!(!d.replay.playing);
        assert_eq!(d.replay.next_at_ms, None);
        assert_eq!(d.replay.queue.len(), 2);
        assert!(d.replay.fearless);
        assert_eq!(d.replay.interval_ms, 7_000);
    }
}
