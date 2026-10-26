//! Opérations pures sur la série.
use super::{CastError, GameRecord, GameSummary, LineupMode, MapStatus, Player, PoolMap, Series};
use crate::draft::Side;

impl Series {
    /// Remplace le pool (ordre donné) ; les cartes conservées gardent leur statut, les autres sont retirées.
    pub fn set_pool(&mut self, names: &[String]) {
        let mut pool: Vec<PoolMap> = Vec::with_capacity(names.len());
        for n in names {
            if pool.iter().any(|m| &m.name == n) {
                continue;
            }
            let status = self.pool.iter().find(|m| &m.name == n).map(|m| m.status).unwrap_or_default();
            pool.push(PoolMap { name: n.clone(), status });
        }
        self.pool = pool;
    }

    pub fn mark_map(&mut self, name: &str, status: MapStatus) -> Result<(), CastError> {
        let idx = self.pool.iter().position(|m| m.name == name).ok_or(CastError::NotInPool)?;
        if self.is_played(name) {
            return Err(CastError::MapAlreadyPlayed);
        }
        if status == MapStatus::Decider
            && self.pool.iter().enumerate().any(|(i, m)| i != idx && m.status == MapStatus::Decider)
        {
            return Err(CastError::DuplicateDecider);
        }
        self.pool[idx].status = status;
        Ok(())
    }

    fn is_chosen(&self, name: &str) -> bool {
        self.pool
            .iter()
            .any(|m| m.name == name && matches!(m.status, MapStatus::Picked { .. } | MapStatus::Decider))
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
        let number = self.games.iter().map(|g| g.number).max().unwrap_or(0).saturating_add(1);
        self.games.push(GameRecord { number, map: map.to_string(), ..Default::default() });
        Ok(number)
    }

    pub fn set_winner(&mut self, number: u8, side: Side) -> Result<(), CastError> {
        let g = self.games.iter_mut().find(|g| g.number == number).ok_or(CastError::UnknownGame)?;
        if g.winner.is_some() {
            return Err(CastError::WinnerAlreadySet);
        }
        g.winner = Some(side);
        Ok(())
    }

    /// Remplit la partie `number` depuis le résumé de son replay (qui fait foi : remplace un
    /// vainqueur/takedowns/durée saisis à la main, et un résumé précédent).
    fn fill_from_summary(&mut self, number: u8, summary: GameSummary) -> Result<(), CastError> {
        let g = self.games.iter_mut().find(|g| g.number == number).ok_or(CastError::UnknownGame)?;
        g.winner = Some(summary.winner);
        g.takedowns = summary.takedowns;
        g.duration_secs = Some(summary.duration_secs);
        g.summary = Some(summary);
        Ok(())
    }

    /// Orientation d'un résumé par rapport à la série (R16) : chaque ligne joueur est rattachée à
    /// un joueur de `blue` ou `red` par la règle R14 (toon lié identique, puis pseudo exact trim /
    /// insensible à la casse — sauf si les deux toons existent et diffèrent). `Some(true)` = l'équipe
    /// 0 du replay est la Red de la série (à échanger), `Some(false)` = orientation directe, `None` =
    /// pas de preuve nette (aucun rattachement, ou autant dans les deux sens).
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

    /// Remplit la partie `number` depuis le résumé de son replay, orienté (R16) : `force_swap`
    /// (choix de l'opérateur) ou preuve par les joueurs → `swapped()` ; sans preuve, un vainqueur
    /// déjà saisi qui contredit le replay → `SideMismatch` (rien ne change) ; sinon équipe 0 = Blue.
    /// Renvoie le résumé tel qu'enregistré.
    pub fn fill_from_replay(&mut self, number: u8, summary: GameSummary, force_swap: bool) -> Result<GameSummary, CastError> {
        let manual = self.games.iter().find(|g| g.number == number).ok_or(CastError::UnknownGame)?.winner;
        let swap = match (force_swap, self.orient(&summary)) {
            (true, _) => true,
            (false, Some(swap)) => swap,
            (false, None) => {
                if manual.is_some_and(|w| w != summary.winner) {
                    return Err(CastError::SideMismatch);
                }
                false
            }
        };
        let s = if swap { summary.swapped() } else { summary };
        self.fill_from_summary(number, s.clone())?;
        Ok(s)
    }

    /// [blue, red] : override si présent, sinon victoires comptées.
    pub fn score(&self) -> [u8; 2] {
        if let Some(s) = self.score_override {
            return s;
        }
        let wins = |side| self.games.iter().filter(|g| g.winner == Some(side)).count() as u8;
        [wins(Side::Blue), wins(Side::Red)]
    }

    /// Première carte pickée/decider (ordre du pool) sans partie associée.
    pub fn next_map(&self) -> Option<&str> {
        self.pool
            .iter()
            .find(|m| self.is_chosen(&m.name) && !self.is_played(&m.name))
            .map(|m| m.name.as_str())
    }

    /// `A` ou `B` : en `Auto`, A avant la 1re partie terminée, B ensuite.
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
        s.mark_map("A", MapStatus::Banned { by: Side::Red }).unwrap();
        s.mark_map("B", MapStatus::Picked { by: Side::Blue }).unwrap();
        s.set_pool(&names(&["B", "D", "A"]));
        let order: Vec<_> = s.pool.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(order, ["B", "D", "A"]);
        assert_eq!(status(&s, "B"), MapStatus::Picked { by: Side::Blue });
        assert_eq!(status(&s, "D"), MapStatus::Available);
        assert_eq!(status(&s, "A"), MapStatus::Banned { by: Side::Red });
    }

    #[test]
    fn mark_map_rejects_map_outside_pool() {
        let mut s = series(&["A"]);
        assert_eq!(s.mark_map("Z", MapStatus::Decider), Err(CastError::NotInPool));
    }

    #[test]
    fn second_decider_is_rejected() {
        let mut s = series(&["A", "B"]);
        s.mark_map("A", MapStatus::Decider).unwrap();
        let before = s.clone();
        assert_eq!(s.mark_map("B", MapStatus::Decider), Err(CastError::DuplicateDecider));
        assert_eq!(s, before);
        s.mark_map("A", MapStatus::Decider).unwrap();
    }

    #[test]
    fn mark_map_refuses_a_map_that_already_has_a_game() {
        let mut s = with_game();
        let before = s.clone();
        assert_eq!(s.mark_map("A", MapStatus::Banned { by: Side::Red }), Err(CastError::MapAlreadyPlayed));
        assert_eq!(s, before);
    }

    #[test]
    fn marking_banned_then_picked_replaces_status() {
        let mut s = series(&["A"]);
        s.mark_map("A", MapStatus::Banned { by: Side::Blue }).unwrap();
        s.mark_map("A", MapStatus::Picked { by: Side::Red }).unwrap();
        assert_eq!(status(&s, "A"), MapStatus::Picked { by: Side::Red });
    }

    #[test]
    fn start_game_requires_picked_or_decider() {
        let mut s = series(&["A", "B", "C", "D"]);
        s.mark_map("B", MapStatus::Banned { by: Side::Blue }).unwrap();
        s.mark_map("C", MapStatus::Picked { by: Side::Blue }).unwrap();
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
        s.mark_map("A", MapStatus::Picked { by: Side::Blue }).unwrap();
        s.mark_map("B", MapStatus::Picked { by: Side::Red }).unwrap();
        assert_eq!(s.start_game("A"), Ok(1));
        assert_eq!(s.start_game("A"), Err(CastError::MapNotChosen));
        assert_eq!(s.start_game("B"), Ok(2));
    }

    fn with_game() -> Series {
        let mut s = series(&["A"]);
        s.mark_map("A", MapStatus::Picked { by: Side::Blue }).unwrap();
        s.start_game("A").unwrap();
        s
    }

    #[test]
    fn set_winner_unknown_game() {
        let mut s = with_game();
        assert_eq!(s.set_winner(9, Side::Blue), Err(CastError::UnknownGame));
    }

    #[test]
    fn set_winner_twice() {
        let mut s = with_game();
        s.set_winner(1, Side::Blue).unwrap();
        assert_eq!(s.set_winner(1, Side::Red), Err(CastError::WinnerAlreadySet));
    }

    #[test]
    fn score_counts_winners_unless_overridden() {
        let mut s = series(&["A", "B", "C"]);
        for n in ["A", "B", "C"] {
            s.mark_map(n, MapStatus::Picked { by: Side::Blue }).unwrap();
            let g = s.start_game(n).unwrap();
            s.set_winner(g, if n == "B" { Side::Red } else { Side::Blue }).unwrap();
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
        s.mark_map("B", MapStatus::Picked { by: Side::Red }).unwrap();
        s.mark_map("A", MapStatus::Banned { by: Side::Red }).unwrap();
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
        s.mark_map("A", MapStatus::Picked { by: Side::Red }).unwrap();
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
            side, toon: toon.into(), name: name.into(), hero: format!("H-{name}"), kills: 0, takedowns: 0, deaths: 0,
            assists: 0, hero_damage: 0, siege_damage: 0, healing: 0,
        }
    }

    /// Résumé où l'équipe 0 du replay (Blue) a gagné, joueurs `a*` en équipe 0 et `b*` en équipe 1.
    fn replay_summary() -> GameSummary {
        GameSummary {
            duration_secs: 600, winner: Side::Blue, takedowns: [10, 4], team_level: Some([20, 18]), structures: Some([3, 1]),
            merc_camps: Some([5, 2]), objectives: None, bans: [vec!["X".into()], vec!["Y".into()]],
            players: vec![row(Side::Blue, "ta1", "Alpha1"), row(Side::Blue, "ta2", "Alpha2"), row(Side::Red, "tb1", "Beta1"), row(Side::Red, "tb2", "Beta2")],
            xp_lead: vec![(60, 300)],
        }
    }

    fn team(players: &[(Option<&str>, &str)]) -> crate::cast::Team {
        let mut t = crate::cast::Team::default();
        for (i, (toon, name)) in players.iter().enumerate() {
            t.players[i] = Player { name: (*name).into(), tagline: String::new(), toon: toon.map(Into::into) };
        }
        t
    }

    fn replay_game(blue: crate::cast::Team, red: crate::cast::Team, winner: Option<Side>) -> Series {
        let mut s = series(&["A"]);
        s.blue = blue;
        s.red = red;
        s.mark_map("A", MapStatus::Picked { by: Side::Blue }).unwrap();
        s.start_game("A").unwrap();
        s.games[0].winner = winner;
        s
    }

    #[test]
    fn orient_swaps_on_toon_evidence() {
        // la Blue de la série joue en équipe 1 du replay (toons tb*)
        let mut s = replay_game(team(&[(Some("tb1"), "x"), (Some("tb2"), "y")]), team(&[(Some("ta1"), "z")]), None);
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
        let mut s = replay_game(team(&[(Some("ta1"), "x")]), team(&[(Some("tb1"), "y")]), Some(Side::Red));
        assert_eq!(s.orient(&replay_summary()), Some(false));
        // preuve nette : le replay fait foi, même contre un vainqueur saisi
        s.fill_from_replay(1, replay_summary(), false).unwrap();
        assert_eq!((s.games[0].winner, s.games[0].takedowns), (Some(Side::Blue), [10, 4]));
    }

    #[test]
    fn orient_swaps_on_name_evidence() {
        let s = replay_game(team(&[(None, " beta1 "), (None, "BETA2")]), team(&[(None, "alpha1")]), None);
        assert_eq!(s.orient(&replay_summary()), Some(true));
        // même pseudo mais toons différents : pas une preuve (homonyme d'un autre compte)
        let s = replay_game(team(&[(Some("other"), "Beta1")]), team(&[]), None);
        assert_eq!(s.orient(&replay_summary()), None);
    }

    #[test]
    fn ambiguous_with_conflicting_manual_winner_is_side_mismatch() {
        let mut s = replay_game(team(&[(None, "Nobody")]), team(&[]), Some(Side::Red));
        assert_eq!(s.orient(&replay_summary()), None);
        let before = s.clone();
        assert_eq!(s.fill_from_replay(1, replay_summary(), false), Err(CastError::SideMismatch));
        assert_eq!(s, before);
        // choix explicite de l'opérateur : échange forcé, accepté
        s.fill_from_replay(1, replay_summary(), true).unwrap();
        assert_eq!(s.games[0].winner, Some(Side::Red));
    }

    #[test]
    fn ambiguous_without_winner_keeps_team0_blue() {
        let mut s = replay_game(team(&[]), team(&[]), None);
        s.fill_from_replay(1, replay_summary(), false).unwrap();
        assert_eq!((s.games[0].winner, s.games[0].takedowns), (Some(Side::Blue), [10, 4]));
        // vainqueur saisi concordant : pas d'erreur
        let mut s = replay_game(team(&[]), team(&[]), Some(Side::Blue));
        assert!(s.fill_from_replay(1, replay_summary(), false).is_ok());
    }
}
