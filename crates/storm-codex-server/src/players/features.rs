//! Pure computations of the similarity indices between two accounts (no DB). Feed
//! `similarity::PairFeatures`; tested in isolation.

use serde_json::Value as J;
use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;

/// Cosine of two count distributions (heroes, hours…), 0..1. 0 if either is empty.
pub fn cosine<K: Hash + Eq>(a: &HashMap<K, u32>, b: &HashMap<K, u32>) -> f32 {
    let dot: f64 = a
        .iter()
        .filter_map(|(k, x)| b.get(k).map(|y| f64::from(*x) * f64::from(*y)))
        .sum();
    let na: f64 = a
        .values()
        .map(|x| f64::from(*x).powi(2))
        .sum::<f64>()
        .sqrt();
    let nb: f64 = b
        .values()
        .map(|x| f64::from(*x).powi(2))
        .sum::<f64>()
        .sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        (dot / (na * nb)).clamp(0.0, 1.0) as f32
    }
}

/// Do two games overlap in time? `(start, end)` intervals in seconds,
/// closed. The same human does not play two games at the same time: an overlap excludes the link.
pub fn intervals_overlap(a: &[(i64, i64)], b: &[(i64, i64)]) -> bool {
    let (mut a, mut b) = (a.to_vec(), b.to_vec());
    a.sort_unstable();
    b.sort_unstable();
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i].0 <= b[j].1 && b[j].0 <= a[i].1 {
            return true;
        }
        // advance the one that ends first
        if a[i].1 < b[j].1 {
            i += 1;
        } else {
            j += 1;
        }
    }
    false
}

/// Games played and most played talent choice per tier (`Tier1Choice`…), for one hero.
/// Tier → (choice → number of games).
type TierCounts = BTreeMap<String, HashMap<String, u32>>;

type HeroTalents = (u32, BTreeMap<String, String>);

fn modal_talents(games: &[(String, J)]) -> HashMap<&str, HeroTalents> {
    let mut counts: HashMap<&str, (u32, TierCounts)> = HashMap::new();
    for (hero, talents) in games {
        let Some(obj) = talents.as_object() else {
            continue;
        };
        let e = counts.entry(hero.as_str()).or_default();
        e.0 += 1;
        for (tier, choice) in obj {
            if let Some(c) = choice
                .as_str()
                .filter(|_| tier.starts_with("Tier") && tier.ends_with("Choice"))
            {
                *e.1.entry(tier.clone())
                    .or_default()
                    .entry(c.to_owned())
                    .or_default() += 1;
            }
        }
    }
    counts
        .into_iter()
        .map(|(h, (n, tiers))| {
            let modal = tiers
                .into_iter()
                .filter_map(|(tier, cs)| {
                    // tie → lexicographically smallest choice (deterministic)
                    cs.into_iter()
                        .max_by(|x, y| x.1.cmp(&y.1).then(y.0.cmp(&x.0)))
                        .map(|(c, _)| (tier, c))
                })
                .collect();
            (h, (n, modal))
        })
        .collect()
}

/// Talent agreement on the heroes played at least `MIN_HERO_GAMES` times by both accounts:
/// share of the tiers where the most played choice is identical. `None` if no hero qualifies.
pub fn talent_agreement(a: &[(String, J)], b: &[(String, J)]) -> Option<f32> {
    const MIN_HERO_GAMES: u32 = 3;
    let (ma, mb) = (modal_talents(a), modal_talents(b));
    let (mut same, mut total) = (0u32, 0u32);
    for (hero, (na, ta)) in &ma {
        let Some((nb, tb)) = mb.get(hero) else {
            continue;
        };
        if *na < MIN_HERO_GAMES || *nb < MIN_HERO_GAMES {
            continue;
        }
        for (tier, ca) in ta {
            if let Some(cb) = tb.get(tier) {
                total += 1;
                same += u32::from(ca == cb);
            }
        }
    }
    (total > 0).then(|| same as f32 / total as f32)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use serde_json::json;

    fn m(pairs: &[(&str, u32)]) -> HashMap<String, u32> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect()
    }

    #[test]
    fn cosine_identique_orthogonal_et_vide() {
        let a = m(&[("Muradin", 10), ("Kerrigan", 5)]);
        assert!((cosine(&a, &a) - 1.0).abs() < 1e-6);
        assert_eq!(cosine(&a, &m(&[("Diablo", 7)])), 0.0);
        assert_eq!(cosine(&a, &HashMap::new()), 0.0);
        // proportional = identical (shares of games, not volumes)
        assert!((cosine(&a, &m(&[("Muradin", 100), ("Kerrigan", 50)])) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn chevauchement_detecte_meme_decale_ou_inclus() {
        assert!(intervals_overlap(&[(0, 100)], &[(50, 150)]));
        assert!(intervals_overlap(&[(0, 1000)], &[(10, 20)])); // inclus
        assert!(intervals_overlap(
            &[(0, 10), (500, 600)],
            &[(20, 30), (550, 560)]
        )); // 2e paire
        assert!(!intervals_overlap(
            &[(0, 10), (100, 110)],
            &[(20, 30), (200, 210)]
        ));
        assert!(!intervals_overlap(&[], &[(0, 1)]));
        assert!(intervals_overlap(&[(5, 5)], &[(5, 5)])); // same instant (e.g. same game, unknown duration)
    }

    fn g(hero: &str, t1: &str, t2: &str) -> (String, J) {
        (
            hero.to_owned(),
            json!({ "Tier1Choice": t1, "Tier2Choice": t2 }),
        )
    }

    #[test]
    fn talents_accord_total_partiel_et_aucun() {
        let a: Vec<_> = (0..3).map(|_| g("Muradin", "A", "B")).collect();
        let same: Vec<_> = (0..4).map(|_| g("Muradin", "A", "B")).collect();
        assert_eq!(talent_agreement(&a, &same), Some(1.0));
        let half: Vec<_> = (0..4).map(|_| g("Muradin", "A", "Z")).collect();
        assert_eq!(talent_agreement(&a, &half), Some(0.5));
        // fewer than 3 games for one of the two → hero ignored → None
        let few: Vec<_> = (0..2).map(|_| g("Muradin", "A", "B")).collect();
        assert_eq!(talent_agreement(&a, &few), None);
        // no hero in common
        assert_eq!(
            talent_agreement(
                &a,
                &(0..3).map(|_| g("Diablo", "A", "B")).collect::<Vec<_>>()
            ),
            None
        );
    }

    #[test]
    fn talents_choix_modal_pas_dernier_choix() {
        // 3 games A / 1 game Q: the modal choice is A
        let mut a: Vec<_> = (0..3).map(|_| g("Muradin", "A", "B")).collect();
        a.push(g("Muradin", "Q", "B"));
        let b: Vec<_> = (0..3).map(|_| g("Muradin", "A", "B")).collect();
        assert_eq!(talent_agreement(&a, &b), Some(1.0));
    }
}
