//! Opérations pures sur la série.
use super::{CastError, GameRecord, LineupMode, MapStatus, PoolMap, Series};
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
}
