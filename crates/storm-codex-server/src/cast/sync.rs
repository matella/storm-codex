//! Synchro à sens unique série → draft : la série de cast est la source de vérité des équipes,
//! pseudos, score et Bo ; le draft (overlay existant) les reçoit sans que sa progression
//! (curseur, assignations, file de replays, fearless) soit touchée.
use super::Series;
use crate::draft::DraftState;

pub fn sync_into_draft(series: &Series, draft: &mut DraftState) {
    for (src, dst) in [(&series.blue, &mut draft.blue), (&series.red, &mut draft.red)] {
        dst.name = src.name.clone();
        for (p, d) in src.players.iter().zip(dst.players.iter_mut()) {
            *d = p.name.clone();
        }
    }
    draft.score = series.score();
    draft.bo = series.bo;
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::cast::{Player, Team};
    use crate::draft::{Format, Side};

    fn player(n: &str) -> Player {
        Player { name: n.into(), ..Player::default() }
    }

    fn series() -> Series {
        let mut s = Series::new();
        s.blue = Team { name: "Blue Co".into(), players: [player("b1"), player("b2"), player("b3"), player("b4"), player("b5")], ..Team::default() };
        s.red = Team { name: "Red Co".into(), players: [player("r1"), player("r2"), player("r3"), player("r4"), player("")], ..Team::default() };
        s.bo = 7;
        s.score_override = Some([2, 1]);
        s
    }

    #[test]
    fn sync_copies_names_players_score_bo() {
        let mut d = DraftState::new(Format::Fearless, Side::Blue, "Sky Temple".into());
        sync_into_draft(&series(), &mut d);
        assert_eq!(d.blue.name, "Blue Co");
        assert_eq!(d.red.name, "Red Co");
        assert_eq!(d.blue.players, ["b1", "b2", "b3", "b4", "b5"].map(String::from));
        assert_eq!(d.red.players, ["r1", "r2", "r3", "r4", ""].map(String::from));
        assert_eq!(d.score, [2, 1]);
        assert_eq!(d.bo, 7);
    }

    #[test]
    fn sync_leaves_draft_progress_untouched() {
        let mut d = DraftState::new(Format::Standard, Side::Red, "Sky Temple".into());
        d.apply("Muradin").unwrap();
        d.apply("Valla").unwrap();
        let before = d.clone();
        sync_into_draft(&series(), &mut d);
        assert_eq!(d.cursor, before.cursor);
        assert_eq!(d.assignments, before.assignments);
        assert_eq!(d.replay.queue.len(), before.replay.queue.len());
        assert_eq!(serde_json::to_value(&d.replay).unwrap(), serde_json::to_value(&before.replay).unwrap());
        assert_eq!(d.format, before.format);
        assert_eq!(d.first_pick, before.first_pick);
        assert_eq!(d.map, before.map);
        assert_eq!(d.series_games, before.series_games);
        assert_eq!(d.series_bans, before.series_bans);
    }
}
