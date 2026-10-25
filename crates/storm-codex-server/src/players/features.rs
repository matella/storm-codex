//! Calculs purs des indices de similarité entre deux comptes (aucune DB). Alimentent
//! `similarity::PairFeatures` ; testés isolément.

use serde_json::Value as J;
use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;

/// Cosinus de deux distributions de comptes (héros, heures…), 0..1. 0 si l'une est vide.
pub fn cosine<K: Hash + Eq>(a: &HashMap<K, u32>, b: &HashMap<K, u32>) -> f32 {
    let dot: f64 = a.iter().filter_map(|(k, x)| b.get(k).map(|y| f64::from(*x) * f64::from(*y))).sum();
    let na: f64 = a.values().map(|x| f64::from(*x).powi(2)).sum::<f64>().sqrt();
    let nb: f64 = b.values().map(|x| f64::from(*x).powi(2)).sum::<f64>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        (dot / (na * nb)).clamp(0.0, 1.0) as f32
    }
}

/// Deux parties se chevauchent-elles dans le temps ? Intervalles `(début, fin)` en secondes,
/// fermés. Un même humain ne joue pas deux parties en même temps : un chevauchement exclut le lien.
pub fn intervals_overlap(a: &[(i64, i64)], b: &[(i64, i64)]) -> bool {
    let (mut a, mut b) = (a.to_vec(), b.to_vec());
    a.sort_unstable();
    b.sort_unstable();
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i].0 <= b[j].1 && b[j].0 <= a[i].1 {
            return true;
        }
        // avancer celui qui se termine le premier
        if a[i].1 < b[j].1 {
            i += 1;
        } else {
            j += 1;
        }
    }
    false
}

/// Parties jouées et choix de talent le plus joué par palier (`Tier1Choice`…), pour un héros.
/// Palier → (choix → nombre de parties).
type TierCounts = BTreeMap<String, HashMap<String, u32>>;

type HeroTalents = (u32, BTreeMap<String, String>);

fn modal_talents(games: &[(String, J)]) -> HashMap<&str, HeroTalents> {
    let mut counts: HashMap<&str, (u32, TierCounts)> = HashMap::new();
    for (hero, talents) in games {
        let Some(obj) = talents.as_object() else { continue };
        let e = counts.entry(hero.as_str()).or_default();
        e.0 += 1;
        for (tier, choice) in obj {
            if let Some(c) = choice.as_str().filter(|_| tier.starts_with("Tier") && tier.ends_with("Choice")) {
                *e.1.entry(tier.clone()).or_default().entry(c.to_owned()).or_default() += 1;
            }
        }
    }
    counts
        .into_iter()
        .map(|(h, (n, tiers))| {
            let modal = tiers
                .into_iter()
                .filter_map(|(tier, cs)| {
                    // égalité → choix lexicographiquement le plus petit (déterministe)
                    cs.into_iter().max_by(|x, y| x.1.cmp(&y.1).then(y.0.cmp(&x.0))).map(|(c, _)| (tier, c))
                })
                .collect();
            (h, (n, modal))
        })
        .collect()
}

/// Accord des talents sur les héros joués au moins `MIN_HERO_GAMES` fois par les deux comptes :
/// part des paliers où le choix le plus joué est identique. `None` si aucun héros ne qualifie.
pub fn talent_agreement(a: &[(String, J)], b: &[(String, J)]) -> Option<f32> {
    const MIN_HERO_GAMES: u32 = 3;
    let (ma, mb) = (modal_talents(a), modal_talents(b));
    let (mut same, mut total) = (0u32, 0u32);
    for (hero, (na, ta)) in &ma {
        let Some((nb, tb)) = mb.get(hero) else { continue };
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
        // proportionnel = identique (parts de parties, pas volumes)
        assert!((cosine(&a, &m(&[("Muradin", 100), ("Kerrigan", 50)])) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn chevauchement_detecte_meme_decale_ou_inclus() {
        assert!(intervals_overlap(&[(0, 100)], &[(50, 150)]));
        assert!(intervals_overlap(&[(0, 1000)], &[(10, 20)])); // inclus
        assert!(intervals_overlap(&[(0, 10), (500, 600)], &[(20, 30), (550, 560)])); // 2e paire
        assert!(!intervals_overlap(&[(0, 10), (100, 110)], &[(20, 30), (200, 210)]));
        assert!(!intervals_overlap(&[], &[(0, 1)]));
        assert!(intervals_overlap(&[(5, 5)], &[(5, 5)])); // même instant (ex. même partie, durée inconnue)
    }

    fn g(hero: &str, t1: &str, t2: &str) -> (String, J) {
        (hero.to_owned(), json!({ "Tier1Choice": t1, "Tier2Choice": t2 }))
    }

    #[test]
    fn talents_accord_total_partiel_et_aucun() {
        let a: Vec<_> = (0..3).map(|_| g("Muradin", "A", "B")).collect();
        let same: Vec<_> = (0..4).map(|_| g("Muradin", "A", "B")).collect();
        assert_eq!(talent_agreement(&a, &same), Some(1.0));
        let half: Vec<_> = (0..4).map(|_| g("Muradin", "A", "Z")).collect();
        assert_eq!(talent_agreement(&a, &half), Some(0.5));
        // moins de 3 parties chez l'un des deux → héros ignoré → None
        let few: Vec<_> = (0..2).map(|_| g("Muradin", "A", "B")).collect();
        assert_eq!(talent_agreement(&a, &few), None);
        // aucun héros en commun
        assert_eq!(talent_agreement(&a, &(0..3).map(|_| g("Diablo", "A", "B")).collect::<Vec<_>>()), None);
    }

    #[test]
    fn talents_choix_modal_pas_dernier_choix() {
        // 3 parties A / 1 partie Q : le choix modal est A
        let mut a: Vec<_> = (0..3).map(|_| g("Muradin", "A", "B")).collect();
        a.push(g("Muradin", "Q", "B"));
        let b: Vec<_> = (0..3).map(|_| g("Muradin", "A", "B")).collect();
        assert_eq!(talent_agreement(&a, &b), Some(1.0));
    }
}
