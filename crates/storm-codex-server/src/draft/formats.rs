//! Draft sequences = data. Written in order role (First/Second), independent of the
//! blue/red side: `DraftState::side_of` resolves the role into a side via the `first_pick` setting.
use crate::draft::{Action, Format, Order, Step};
use Action::{Ban, Pick};
use Order::{First, Second};

// Standard HotS sequence (confirmed by the operator). First = first-pick team.
// ban F · ban S · ban F · ban S · pick F · pick S×2 · pick F×2 · ban S · ban F · pick S×2 · pick F×2 · pick S.
const STANDARD: [(Order, Action); 16] = [
    (First, Ban),
    (Second, Ban),
    (First, Ban),
    (Second, Ban),
    (First, Pick),
    (Second, Pick),
    (Second, Pick),
    (First, Pick),
    (First, Pick),
    (Second, Ban),
    (First, Ban),
    (Second, Pick),
    (Second, Pick),
    (First, Pick),
    (First, Pick),
    (Second, Pick),
];

/// Steps of a format. Normal = Standard without the bans; Fearless = Standard (the series bans are
/// handled by the state, not by the sequence).
pub fn steps_for(format: Format) -> Vec<Step> {
    STANDARD
        .iter()
        .filter(|(_, a)| format != Format::Normal || *a == Pick)
        .map(|&(order, action)| Step { order, action })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(steps: &[Step]) -> (usize, usize, usize, usize) {
        let bf = steps
            .iter()
            .filter(|s| s.action == Ban && s.order == First)
            .count();
        let bs = steps
            .iter()
            .filter(|s| s.action == Ban && s.order == Second)
            .count();
        let pf = steps
            .iter()
            .filter(|s| s.action == Pick && s.order == First)
            .count();
        let ps = steps
            .iter()
            .filter(|s| s.action == Pick && s.order == Second)
            .count();
        (bf, bs, pf, ps)
    }

    #[test]
    fn standard_is_3_bans_5_picks_per_role_len_16() {
        let s = steps_for(Format::Standard);
        assert_eq!(counts(&s), (3, 3, 5, 5));
        assert_eq!(s.len(), 16);
    }

    #[test]
    fn standard_exact_order() {
        let s = steps_for(Format::Standard);
        let got: Vec<(Order, Action)> = s.iter().map(|x| (x.order, x.action)).collect();
        assert_eq!(
            got,
            vec![
                (First, Ban),
                (Second, Ban),
                (First, Ban),
                (Second, Ban),
                (First, Pick),
                (Second, Pick),
                (Second, Pick),
                (First, Pick),
                (First, Pick),
                (Second, Ban),
                (First, Ban),
                (Second, Pick),
                (Second, Pick),
                (First, Pick),
                (First, Pick),
                (Second, Pick),
            ]
        );
    }

    #[test]
    fn normal_has_no_bans_10_picks() {
        let s = steps_for(Format::Normal);
        assert_eq!(counts(&s), (0, 0, 5, 5));
        assert_eq!(s.len(), 10);
    }

    #[test]
    fn fearless_sequence_equals_standard() {
        assert_eq!(steps_for(Format::Fearless), steps_for(Format::Standard));
    }
}
