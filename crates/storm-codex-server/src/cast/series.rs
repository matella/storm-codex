//! Pure operations on the series.
use super::{CastError, GameRecord, GameSummary, LineupMode, MapStatus, Player, PoolMap, Series};
use crate::draft::Side;
use std::collections::{HashMap, HashSet};

/// How the orientation of a replay was decided (R16): evidence from the linked players, assumption
/// team 0 = Blue for lack of evidence, or swap forced by the operator (`?swap=1`) — possibly
/// **against** direct orientation evidence.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Orientation {
    Evidence,
    Assumed,
    Forced { against_evidence: bool },
}

impl Orientation {
    pub fn code(self) -> &'static str {
        match self {
            Self::Evidence => "evidence",
            Self::Assumed => "assumed",
            Self::Forced { .. } => "forced",
        }
    }
}

impl Series {
    /// Replaces the pool (given order); kept maps keep their status, the others are removed.
    pub fn set_pool(&mut self, names: &[String]) {
        // Linear (HashMap/HashSet): a huge pool must not cost quadratic time under the lock.
        let old: HashMap<&str, MapStatus> = self
            .pool
            .iter()
            .map(|m| (m.name.as_str(), m.status))
            .collect();
        let mut seen: HashSet<&str> = HashSet::with_capacity(names.len());
        let mut pool: Vec<PoolMap> = Vec::with_capacity(names.len());
        for n in names {
            if !seen.insert(n.as_str()) {
                continue;
            }
            let status = old.get(n.as_str()).copied().unwrap_or_default();
            pool.push(PoolMap {
                name: n.clone(),
                status,
            });
        }
        self.pool = pool;
    }

    pub fn mark_map(&mut self, name: &str, status: MapStatus) -> Result<(), CastError> {
        let idx = self
            .pool
            .iter()
            .position(|m| m.name == name)
            .ok_or(CastError::NotInPool)?;
        if self.is_played(name) {
            return Err(CastError::MapAlreadyPlayed);
        }
        if status == MapStatus::Decider
            && self
                .pool
                .iter()
                .enumerate()
                .any(|(i, m)| i != idx && m.status == MapStatus::Decider)
        {
            return Err(CastError::DuplicateDecider);
        }
        self.pool[idx].status = status;
        Ok(())
    }

    fn is_chosen(&self, name: &str) -> bool {
        self.pool.iter().any(|m| {
            m.name == name && matches!(m.status, MapStatus::Picked { .. } | MapStatus::Decider)
        })
    }

    fn is_played(&self, name: &str) -> bool {
        self.games.iter().any(|g| g.map == name)
    }

    pub fn start_game(&mut self, map: &str) -> Result<u8, CastError> {
        if !self.pool.iter().any(|m| m.name == map) {
            return Err(CastError::UnknownMap);
        }
        if !self.is_chosen(map) || self.is_played(map) {
            return Err(CastError::MapNotChosen);
        }
        let number = self
            .games
            .iter()
            .map(|g| g.number)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        self.games.push(GameRecord {
            number,
            map: map.to_string(),
            ..Default::default()
        });
        Ok(number)
    }

    /// Sets (or corrects, R21: misclick) the winner entered by hand; refused if the game has
    /// a replay summary (it carries its own winner and orientation).
    pub fn set_winner(&mut self, number: u8, side: Side) -> Result<(), CastError> {
        let g = self
            .games
            .iter_mut()
            .find(|g| g.number == number)
            .ok_or(CastError::UnknownGame)?;
        if g.summary.is_some() {
            return Err(CastError::WinnerAlreadySet);
        }
        g.winner = Some(side);
        Ok(())
    }

    /// Removes game `number` launched by mistake (R21): only the last one, and only
    /// without a winner or summary. The map's status (picked/decider) is unchanged: it becomes
    /// launchable again. A game pinned in the slideshow is unpinned.
    pub fn remove_game(&mut self, number: u8) -> Result<(), CastError> {
        let idx = self
            .games
            .iter()
            .position(|g| g.number == number)
            .ok_or(CastError::UnknownGame)?;
        if self.games.iter().any(|g| g.number > number) {
            return Err(CastError::NotLastGame);
        }
        let g = &self.games[idx];
        if g.winner.is_some() || g.summary.is_some() {
            return Err(CastError::GameHasResult);
        }
        self.games.remove(idx);
        if self.slide.game == Some(number) {
            self.slide.game = None;
        }
        Ok(())
    }

    /// Fills game `number` from its replay's summary (which is authoritative: replaces a
    /// winner/takedowns/duration entered by hand, and a previous summary).
    fn fill_from_summary(&mut self, number: u8, summary: GameSummary) -> Result<(), CastError> {
        let g = self
            .games
            .iter_mut()
            .find(|g| g.number == number)
            .ok_or(CastError::UnknownGame)?;
        g.winner = Some(summary.winner);
        g.takedowns = summary.takedowns;
        g.duration_secs = Some(summary.duration_secs);
        g.summary = Some(summary);
        Ok(())
    }

    /// Orientation of a summary relative to the series (R16): each player row is attached to
    /// a `blue` or `red` player by rule R14 (identical linked toon, then exact trimmed /
    /// case-insensitive nickname — unless both toons exist and differ). `Some(true)` = replay team
    /// 0 is the series' Red (to swap), `Some(false)` = direct orientation, `None` =
    /// no clear evidence (no attachment, or as many in both directions). Simple majority:
    /// a row attached in the wrong direction is offset by one in the right direction. A player listed
    /// in BOTH teams of the series (inconsistent input) proves nothing: their row is ignored.
    pub fn orient(&self, s: &GameSummary) -> Option<bool> {
        let matches = |p: &Player, row: &super::PlayerSummary| {
            if let Some(t) = p.toon.as_deref().filter(|t| !t.is_empty()) {
                if !row.toon.is_empty() {
                    return t == row.toon;
                }
            }
            let n = p.name.trim().to_lowercase();
            !n.is_empty() && n == row.name.trim().to_lowercase()
        };
        let in_team = |team: &super::Team, row| team.players.iter().any(|p| matches(p, row));
        let (mut direct, mut crossed) = (0u32, 0u32);
        for row in &s.players {
            let (b, r) = (in_team(&self.blue, row), in_team(&self.red, row));
            match (row.side, b, r) {
                (Side::Blue, true, false) | (Side::Red, false, true) => direct += 1,
                (Side::Blue, false, true) | (Side::Red, true, false) => crossed += 1,
                _ => {}
            }
        }
        match direct.cmp(&crossed) {
            std::cmp::Ordering::Greater => Some(false),
            std::cmp::Ordering::Less => Some(true),
            std::cmp::Ordering::Equal => None,
        }
    }

    /// Fills game `number` from its replay's summary, oriented (R16): `force_swap`
    /// (operator choice) or evidence from the players → `swapped()`; without evidence, an already
    /// entered winner that contradicts the replay → `SideMismatch` (nothing changes); otherwise team 0 = Blue.
    /// Returns the summary as recorded.
    pub fn fill_from_replay(
        &mut self,
        number: u8,
        summary: GameSummary,
        force_swap: bool,
    ) -> Result<GameSummary, CastError> {
        self.fill_from_replay_oriented(number, summary, force_swap)
            .map(|(s, _)| s)
    }

    /// Like `fill_from_replay`, and tells how the orientation was decided (route response).
    pub fn fill_from_replay_oriented(
        &mut self,
        number: u8,
        summary: GameSummary,
        force_swap: bool,
    ) -> Result<(GameSummary, Orientation), CastError> {
        let manual = self
            .games
            .iter()
            .find(|g| g.number == number)
            .ok_or(CastError::UnknownGame)?
            .winner;
        let evidence = self.orient(&summary);
        let (swap, how) = match (force_swap, evidence) {
            // forced swap while the linked players prove the direct orientation
            (true, e) => (
                true,
                Orientation::Forced {
                    against_evidence: e == Some(false),
                },
            ),
            (false, Some(swap)) => (swap, Orientation::Evidence),
            (false, None) => {
                if manual.is_some_and(|w| w != summary.winner) {
                    return Err(CastError::SideMismatch);
                }
                (false, Orientation::Assumed)
            }
        };
        let s = if swap { summary.swapped() } else { summary };
        self.fill_from_summary(number, s.clone())?;
        Ok((s, how))
    }

    /// [blue, red]: override if present, otherwise counted wins.
    pub fn score(&self) -> [u8; 2] {
        if let Some(s) = self.score_override {
            return s;
        }
        let wins = |side| self.games.iter().filter(|g| g.winner == Some(side)).count() as u8;
        [wins(Side::Blue), wins(Side::Red)]
    }

    /// First picked/decider map (pool order) without an associated game.
    pub fn next_map(&self) -> Option<&str> {
        self.pool
            .iter()
            .find(|m| self.is_chosen(&m.name) && !self.is_played(&m.name))
            .map(|m| m.name.as_str())
    }

    /// `A` or `B`: in `Auto`, A before the 1st finished game, B afterwards.
    pub fn lineup_mode_effective(&self) -> LineupMode {
        match self.lineup_mode {
            LineupMode::Auto => {
                if self.games.iter().any(|g| g.winner.is_some()) {
                    LineupMode::B
                } else {
                    LineupMode::A
                }
            }
            forced => forced,
        }
    }

    pub fn display_name(&self, side: Side) -> &str {
        let (team, default) = match side {
            Side::Blue => (&self.blue, "Blue"),
            Side::Red => (&self.red, "Red"),
        };
        if team.name.trim().is_empty() {
            default
        } else {
            &team.name
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }
    fn series(maps: &[&str]) -> Series {
        let mut s = Series::new();
        s.set_pool(&names(maps));
        s
    }
    fn status(s: &Series, n: &str) -> MapStatus {
        s.pool.iter().find(|m| m.name == n).unwrap().status
    }

    #[test]
    fn set_pool_keeps_status_of_retained_maps_and_drops_others() {
        let mut s = series(&["A", "B", "C"]);
        s.mark_map("A", MapStatus::Banned { by: Side::Red })
            .unwrap();
        s.mark_map("B", MapStatus::Picked { by: Side::Blue })
            .unwrap();
        s.set_pool(&names(&["B", "D", "A"]));
        let order: Vec<_> = s.pool.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(order, ["B", "D", "A"]);
        assert_eq!(status(&s, "B"), MapStatus::Picked { by: Side::Blue });
        assert_eq!(status(&s, "D"), MapStatus::Available);
        assert_eq!(status(&s, "A"), MapStatus::Banned { by: Side::Red });
    }

    #[test]
    fn set_pool_dedups_a_huge_list_and_keeps_first_occurrence_order() {
        let mut s = series(&["A"]);
        s.mark_map("A", MapStatus::Banned { by: Side::Red })
            .unwrap();
        let mut names: Vec<String> = (0..50_000).map(|_| "A".to_string()).collect();
        names.extend((0..50_000).map(|i| format!("M{}", i % 3)));
        s.set_pool(&names);
        let order: Vec<_> = s.pool.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(order, ["A", "M0", "M1", "M2"]);
        assert_eq!(status(&s, "A"), MapStatus::Banned { by: Side::Red });
    }

    #[test]
    fn mark_map_rejects_map_outside_pool() {
        let mut s = series(&["A"]);
        assert_eq!(
            s.mark_map("Z", MapStatus::Decider),
            Err(CastError::NotInPool)
        );
    }

    #[test]
    fn second_decider_is_rejected() {
        let mut s = series(&["A", "B"]);
        s.mark_map("A", MapStatus::Decider).unwrap();
        let before = s.clone();
        assert_eq!(
            s.mark_map("B", MapStatus::Decider),
            Err(CastError::DuplicateDecider)
        );
        assert_eq!(s, before);
        s.mark_map("A", MapStatus::Decider).unwrap();
    }

    #[test]
    fn mark_map_refuses_a_map_that_already_has_a_game() {
        let mut s = with_game();
        let before = s.clone();
        assert_eq!(
            s.mark_map("A", MapStatus::Banned { by: Side::Red }),
            Err(CastError::MapAlreadyPlayed)
        );
        assert_eq!(s, before);
    }

    #[test]
    fn marking_banned_then_picked_replaces_status() {
        let mut s = series(&["A"]);
        s.mark_map("A", MapStatus::Banned { by: Side::Blue })
            .unwrap();
        s.mark_map("A", MapStatus::Picked { by: Side::Red })
            .unwrap();
        assert_eq!(status(&s, "A"), MapStatus::Picked { by: Side::Red });
    }

    #[test]
    fn start_game_requires_picked_or_decider() {
        let mut s = series(&["A", "B", "C", "D"]);
        s.mark_map("B", MapStatus::Banned { by: Side::Blue })
            .unwrap();
        s.mark_map("C", MapStatus::Picked { by: Side::Blue })
            .unwrap();
        s.mark_map("D", MapStatus::Decider).unwrap();
        assert_eq!(s.start_game("A"), Err(CastError::MapNotChosen));
        assert_eq!(s.start_game("B"), Err(CastError::MapNotChosen));
        assert_eq!(s.start_game("Z"), Err(CastError::UnknownMap));
        assert!(s.start_game("C").is_ok());
        assert!(s.start_game("D").is_ok());
    }

    #[test]
    fn start_game_numbers_games_from_one_and_rejects_replaying_a_map() {
        let mut s = series(&["A", "B"]);
        s.mark_map("A", MapStatus::Picked { by: Side::Blue })
            .unwrap();
        s.mark_map("B", MapStatus::Picked { by: Side::Red })
            .unwrap();
        assert_eq!(s.start_game("A"), Ok(1));
        assert_eq!(s.start_game("A"), Err(CastError::MapNotChosen));
        assert_eq!(s.start_game("B"), Ok(2));
    }

    fn with_game() -> Series {
        let mut s = series(&["A"]);
        s.mark_map("A", MapStatus::Picked { by: Side::Blue })
            .unwrap();
        s.start_game("A").unwrap();
        s
    }

    #[test]
    fn set_winner_unknown_game() {
        let mut s = with_game();
        assert_eq!(s.set_winner(9, Side::Blue), Err(CastError::UnknownGame));
    }

    /// R21: without a summary, a second winner corrects the first (score and lineup follow);
    /// with a replay summary, `WinnerAlreadySet` refusal, state unchanged.
    #[test]
    fn set_winner_overwrites_manual_winner_but_not_replay_summary() {
        let mut s = with_game();
        s.set_winner(1, Side::Blue).unwrap();
        assert_eq!(s.score(), [1, 0]);
        s.set_winner(1, Side::Red).unwrap();
        assert_eq!(s.games[0].winner, Some(Side::Red));
        assert_eq!(s.score(), [0, 1]);
        assert_eq!(s.lineup_mode_effective(), LineupMode::B);
        let mut s = replay_game(team(&[]), team(&[]), None);
        s.fill_from_replay(1, replay_summary(), false).unwrap();
        let before = s.clone();
        assert_eq!(s.set_winner(1, Side::Red), Err(CastError::WinnerAlreadySet));
        assert_eq!(s, before);
    }

    #[test]
    fn remove_game_only_last_without_result_and_map_becomes_startable_again() {
        let mut s = series(&["A", "B"]);
        s.mark_map("A", MapStatus::Picked { by: Side::Blue })
            .unwrap();
        s.mark_map("B", MapStatus::Decider).unwrap();
        s.start_game("A").unwrap();
        s.start_game("B").unwrap();
        s.slide.game = Some(2);
        assert_eq!(s.remove_game(9), Err(CastError::UnknownGame));
        assert_eq!(s.remove_game(1), Err(CastError::NotLastGame));
        s.remove_game(2).unwrap();
        assert_eq!(s.games.len(), 1);
        assert_eq!(s.slide.game, None);
        assert_eq!(status(&s, "B"), MapStatus::Decider);
        assert_eq!(s.next_map(), Some("B"));
        assert_eq!(s.start_game("B"), Ok(2));
        // winner set: not removable
        s.set_winner(2, Side::Red).unwrap();
        let before = s.clone();
        assert_eq!(s.remove_game(2), Err(CastError::GameHasResult));
        assert_eq!(s, before);
        // summary without an entered winner (theoretical case): not removable either
        let mut s = with_game();
        s.games[0].summary = Some(replay_summary());
        assert_eq!(s.remove_game(1), Err(CastError::GameHasResult));
    }

    #[test]
    fn score_counts_winners_unless_overridden() {
        let mut s = series(&["A", "B", "C"]);
        for n in ["A", "B", "C"] {
            s.mark_map(n, MapStatus::Picked { by: Side::Blue }).unwrap();
            let g = s.start_game(n).unwrap();
            s.set_winner(g, if n == "B" { Side::Red } else { Side::Blue })
                .unwrap();
        }
        assert_eq!(s.score(), [2, 1]);
        s.score_override = Some([0, 3]);
        assert_eq!(s.score(), [0, 3]);
    }

    #[test]
    fn next_map_is_first_picked_or_decider_without_a_game() {
        let mut s = series(&["A", "B", "C", "D"]);
        assert_eq!(s.next_map(), None);
        s.mark_map("D", MapStatus::Decider).unwrap();
        s.mark_map("B", MapStatus::Picked { by: Side::Red })
            .unwrap();
        s.mark_map("A", MapStatus::Banned { by: Side::Red })
            .unwrap();
        assert_eq!(s.next_map(), Some("B"));
        s.start_game("B").unwrap();
        assert_eq!(s.next_map(), Some("D"));
        s.start_game("D").unwrap();
        assert_eq!(s.next_map(), None);
    }

    #[test]
    fn lineup_mode_auto_switches_to_b_after_first_finished_game() {
        let mut s = with_game();
        assert_eq!(s.lineup_mode_effective(), LineupMode::A);
        s.set_winner(1, Side::Red).unwrap();
        assert_eq!(s.lineup_mode_effective(), LineupMode::B);
    }

    #[test]
    fn lineup_mode_forced_wins_over_auto() {
        let mut s = with_game();
        s.set_winner(1, Side::Red).unwrap();
        s.lineup_mode = LineupMode::A;
        assert_eq!(s.lineup_mode_effective(), LineupMode::A);
        s.lineup_mode = LineupMode::B;
        assert_eq!(s.lineup_mode_effective(), LineupMode::B);
    }

    #[test]
    fn display_name_falls_back_to_blue_red_when_blank() {
        let mut s = Series::new();
        assert_eq!(s.display_name(Side::Blue), "Blue");
        assert_eq!(s.display_name(Side::Red), "Red");
        s.blue.name = "Storm".into();
        s.red.name = "   ".into();
        assert_eq!(s.display_name(Side::Blue), "Storm");
        assert_eq!(s.display_name(Side::Red), "Red");
    }

    #[test]
    fn series_roundtrips_json_and_loads_default_for_missing_fields() {
        let mut s = series(&["A", "B"]);
        s.blue.name = "X".into();
        s.mark_map("A", MapStatus::Picked { by: Side::Red })
            .unwrap();
        s.start_game("A").unwrap();
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""status":"picked""#), "{json}");
        assert_eq!(serde_json::from_str::<Series>(&json).unwrap(), s);

        let d: Series = serde_json::from_str("{}").unwrap();
        assert_eq!(d, Series::new());
        let p: Series = serde_json::from_str(r#"{"bo":3,"pool":[{"name":"A"}]}"#).unwrap();
        assert_eq!(p.bo, 3);
        assert_eq!(p.pool[0].status, MapStatus::Available);
        assert_eq!(p.slide.interval_ms, 12_000);
    }

    fn row(side: Side, toon: &str, name: &str) -> crate::cast::PlayerSummary {
        crate::cast::PlayerSummary {
            side,
            toon: toon.into(),
            name: name.into(),
            hero: format!("H-{name}"),
            kills: 0,
            takedowns: 0,
            deaths: 0,
            assists: 0,
            hero_damage: 0,
            siege_damage: 0,
            healing: 0,
        }
    }

    /// Summary where replay team 0 (Blue) won, players `a*` in team 0 and `b*` in team 1.
    fn replay_summary() -> GameSummary {
        GameSummary {
            map: None,
            played_at: None,
            duration_secs: 600,
            winner: Side::Blue,
            takedowns: [10, 4],
            team_level: Some([20, 18]),
            structures: Some([3, 1]),
            merc_camps: Some([5, 2]),
            objectives: None,
            bans: [vec!["X".into()], vec!["Y".into()]],
            players: vec![
                row(Side::Blue, "ta1", "Alpha1"),
                row(Side::Blue, "ta2", "Alpha2"),
                row(Side::Red, "tb1", "Beta1"),
                row(Side::Red, "tb2", "Beta2"),
            ],
            xp_lead: vec![(60, 300)],
        }
    }

    fn team(players: &[(Option<&str>, &str)]) -> crate::cast::Team {
        let mut t = crate::cast::Team::default();
        for (i, (toon, name)) in players.iter().enumerate() {
            t.players[i] = Player {
                name: (*name).into(),
                tagline: String::new(),
                toon: toon.map(Into::into),
            };
        }
        t
    }

    fn replay_game(
        blue: crate::cast::Team,
        red: crate::cast::Team,
        winner: Option<Side>,
    ) -> Series {
        let mut s = series(&["A"]);
        s.blue = blue;
        s.red = red;
        s.mark_map("A", MapStatus::Picked { by: Side::Blue })
            .unwrap();
        s.start_game("A").unwrap();
        s.games[0].winner = winner;
        s
    }

    #[test]
    fn orient_swaps_on_toon_evidence() {
        // the series' Blue plays in replay team 1 (toons tb*)
        let mut s = replay_game(
            team(&[(Some("tb1"), "x"), (Some("tb2"), "y")]),
            team(&[(Some("ta1"), "z")]),
            None,
        );
        assert_eq!(s.orient(&replay_summary()), Some(true));
        let got = s.fill_from_replay(1, replay_summary(), false).unwrap();
        let g = &s.games[0];
        assert_eq!((g.winner, g.takedowns), (Some(Side::Red), [4, 10]));
        assert_eq!(got.xp_lead, vec![(60, -300)]);
        assert_eq!(got.players[0].toon, "tb1");
        assert_eq!(got.players[0].side, Side::Blue);
    }

    #[test]
    fn orient_keeps_direct_orientation() {
        let mut s = replay_game(
            team(&[(Some("ta1"), "x")]),
            team(&[(Some("tb1"), "y")]),
            Some(Side::Red),
        );
        assert_eq!(s.orient(&replay_summary()), Some(false));
        // clear evidence: the replay is authoritative, even against an entered winner
        s.fill_from_replay(1, replay_summary(), false).unwrap();
        assert_eq!(
            (s.games[0].winner, s.games[0].takedowns),
            (Some(Side::Blue), [10, 4])
        );
    }

    #[test]
    fn orient_swaps_on_name_evidence() {
        let s = replay_game(
            team(&[(None, " beta1 "), (None, "BETA2")]),
            team(&[(None, "alpha1")]),
            None,
        );
        assert_eq!(s.orient(&replay_summary()), Some(true));
        // same nickname but different toons: not evidence (namesake of another account)
        let s = replay_game(team(&[(Some("other"), "Beta1")]), team(&[]), None);
        assert_eq!(s.orient(&replay_summary()), None);
    }

    #[test]
    fn ambiguous_with_conflicting_manual_winner_is_side_mismatch() {
        let mut s = replay_game(team(&[(None, "Nobody")]), team(&[]), Some(Side::Red));
        assert_eq!(s.orient(&replay_summary()), None);
        let before = s.clone();
        assert_eq!(
            s.fill_from_replay(1, replay_summary(), false),
            Err(CastError::SideMismatch)
        );
        assert_eq!(s, before);
        // explicit operator choice: forced swap, accepted
        s.fill_from_replay(1, replay_summary(), true).unwrap();
        assert_eq!(s.games[0].winner, Some(Side::Red));
    }

    /// Mixed evidence: the majority decides, a tie is not evidence.
    #[test]
    fn orient_mixed_evidence_majority_and_tie() {
        // 2 crossed attachments (Beta1, Beta2 in the Blue) against 1 direct (Alpha1 in the Blue) → swap
        let s = replay_game(
            team(&[(None, "Beta1"), (None, "Beta2"), (None, "Alpha1")]),
            team(&[]),
            None,
        );
        assert_eq!(s.orient(&replay_summary()), Some(true));
        // 2 direct (Alpha1, Alpha2) against 1 crossed (Beta1) → direct
        let s = replay_game(
            team(&[(None, "Alpha1"), (None, "Alpha2"), (None, "Beta1")]),
            team(&[]),
            None,
        );
        assert_eq!(s.orient(&replay_summary()), Some(false));
        // 1 against 1 → no evidence; with a contrary entered winner → SideMismatch
        let mut s = replay_game(
            team(&[(None, "Alpha1"), (None, "Beta1")]),
            team(&[]),
            Some(Side::Red),
        );
        assert_eq!(s.orient(&replay_summary()), None);
        assert_eq!(
            s.fill_from_replay(1, replay_summary(), false),
            Err(CastError::SideMismatch)
        );
    }

    /// A player listed in both teams of the series is ignored (not evidence).
    #[test]
    fn orient_ignores_player_listed_in_both_teams() {
        let s = replay_game(team(&[(None, "Beta1")]), team(&[(None, "Beta1")]), None);
        assert_eq!(s.orient(&replay_summary()), None);
        // another player's evidence is still taken into account
        let s = replay_game(
            team(&[(None, "Beta1"), (None, "Beta2")]),
            team(&[(None, "Beta1")]),
            None,
        );
        assert_eq!(s.orient(&replay_summary()), Some(true));
    }

    /// Item 12: the route tells how the orientation was decided.
    #[test]
    fn orientation_reports_evidence_assumed_and_forced() {
        let mut s = replay_game(team(&[(Some("ta1"), "x")]), team(&[]), None);
        assert_eq!(
            s.fill_from_replay_oriented(1, replay_summary(), false)
                .unwrap()
                .1,
            Orientation::Evidence
        );
        let mut s = replay_game(team(&[]), team(&[]), None);
        assert_eq!(
            s.fill_from_replay_oriented(1, replay_summary(), false)
                .unwrap()
                .1,
            Orientation::Assumed
        );
        let mut s = replay_game(team(&[]), team(&[]), None);
        assert_eq!(
            s.fill_from_replay_oriented(1, replay_summary(), true)
                .unwrap()
                .1,
            Orientation::Forced {
                against_evidence: false
            }
        );
        // forced swap while the evidence says "direct"
        let mut s = replay_game(team(&[(Some("ta1"), "x")]), team(&[]), None);
        let (got, how) = s
            .fill_from_replay_oriented(1, replay_summary(), true)
            .unwrap();
        assert_eq!(
            how,
            Orientation::Forced {
                against_evidence: true
            }
        );
        assert_eq!(got.winner, Side::Red);
        // forced swap in the direction of the evidence: not "against"
        let mut s = replay_game(team(&[(Some("tb1"), "x")]), team(&[]), None);
        assert_eq!(
            s.fill_from_replay_oriented(1, replay_summary(), true)
                .unwrap()
                .1,
            Orientation::Forced {
                against_evidence: false
            }
        );
        assert_eq!(
            [
                Orientation::Evidence,
                Orientation::Assumed,
                Orientation::Forced {
                    against_evidence: true
                }
            ]
            .map(Orientation::code),
            ["evidence", "assumed", "forced"]
        );
    }

    #[test]
    fn ambiguous_without_winner_keeps_team0_blue() {
        let mut s = replay_game(team(&[]), team(&[]), None);
        s.fill_from_replay(1, replay_summary(), false).unwrap();
        assert_eq!(
            (s.games[0].winner, s.games[0].takedowns),
            (Some(Side::Blue), [10, 4])
        );
        // concordant entered winner: no error
        let mut s = replay_game(team(&[]), team(&[]), Some(Side::Blue));
        assert!(s.fill_from_replay(1, replay_summary(), false).is_ok());
    }
}
