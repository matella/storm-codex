//! Détection de l'équipe cible (règle opérateur, spec volet B) : dans chaque partie, le côté
//! cible compte au moins 3 joueurs du roster ; sinon le côté d'un joueur ancre ; sinon choix
//! manuel (porté par l'appelant). Identifiant de joueur = `toon_handle`, jamais le nom affiché.

use super::summary::GameSummary;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Seuil de la règle opérateur : « toujours au moins 3 joueurs de l'équipe principale ».
pub const MIN_ROSTER_ON_SIDE: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Roster,
    Anchor,
    Manual,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Roster => "roster",
            Source::Anchor => "anchor",
            Source::Manual => "manual",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Candidate {
    pub toon: String,
    pub name: String,
    pub games: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Detection {
    pub candidates: Vec<Candidate>,
    /// Des candidats se sont affrontés dans une même partie : le lot ne permet pas de dire
    /// quelle équipe est scoutée (typiquement une série contre un même adversaire). Il faut une
    /// ancre.
    pub ambiguous: bool,
}

fn side_toons(g: &GameSummary, side: usize) -> impl Iterator<Item = &str> {
    g.teams[side].iter().map(|p| p.toon.as_str())
}

fn anchor_side(g: &GameSummary, anchors: &BTreeSet<&str>) -> Option<usize> {
    let hit = |s: usize| side_toons(g, s).any(|t| anchors.contains(t));
    match (hit(0), hit(1)) {
        (true, false) => Some(0),
        (false, true) => Some(1),
        _ => None, // absente, ou des deux côtés (donnée incohérente) → ne tranche pas
    }
}

/// Candidats au roster : joueurs présents dans au moins 50 % des parties — ou, si des ancres
/// sont définies, dans au moins 50 % des parties de l'ancre et **du même côté qu'elle**.
pub fn candidates(games: &[GameSummary], anchors: &[String]) -> Detection {
    let anchors: BTreeSet<&str> = anchors.iter().map(String::as_str).collect();
    let mut count: BTreeMap<&str, (u32, &str)> = BTreeMap::new();
    let mut total = 0u32;
    for g in games {
        let sides: Vec<usize> = if anchors.is_empty() {
            vec![0, 1]
        } else {
            match anchor_side(g, &anchors) {
                Some(s) => vec![s],
                None => continue,
            }
        };
        total += 1;
        for s in sides {
            for p in &g.teams[s] {
                let e = count.entry(p.toon.as_str()).or_insert((0, p.name.as_str()));
                e.0 += 1;
            }
        }
    }
    let mut candidates: Vec<Candidate> = count
        .into_iter()
        .filter(|(_, (n, _))| total > 0 && *n * 2 >= total)
        .map(|(toon, (n, name))| Candidate { toon: toon.into(), name: name.into(), games: n })
        .collect();
    candidates.sort_by(|a, b| b.games.cmp(&a.games).then_with(|| a.name.cmp(&b.name)));

    let set: BTreeSet<&str> = candidates.iter().map(|c| c.toon.as_str()).collect();
    let ambiguous = anchors.is_empty()
        && games.iter().any(|g| {
            side_toons(g, 0).any(|t| set.contains(t)) && side_toons(g, 1).any(|t| set.contains(t))
        });
    Detection { candidates, ambiguous }
}

/// Roster effectif : celui fixé par l'opérateur, sinon les candidats détectés (vide si le lot est
/// ambigu — l'UI demande alors une ancre).
pub fn effective_roster(stored: &[String], detection: &Detection) -> Vec<String> {
    if !stored.is_empty() {
        return stored.to_vec();
    }
    if detection.ambiguous {
        return Vec::new();
    }
    detection.candidates.iter().map(|c| c.toon.clone()).collect()
}

/// Côté cible d'une partie par les règles automatiques (le choix manuel est géré par l'appelant,
/// qui le conserve). `None` = aucune règle ne tranche → partie exclue des faits.
pub fn detect(g: &GameSummary, roster: &[String], anchors: &[String]) -> Option<(u8, Source)> {
    let roster: BTreeSet<&str> = roster.iter().map(String::as_str).collect();
    let on = |s: usize| side_toons(g, s).filter(|t| roster.contains(t)).count();
    let (a, b) = (on(0), on(1));
    if a >= MIN_ROSTER_ON_SIDE && b < MIN_ROSTER_ON_SIDE {
        return Some((0, Source::Roster));
    }
    if b >= MIN_ROSTER_ON_SIDE && a < MIN_ROSTER_ON_SIDE {
        return Some((1, Source::Roster));
    }
    let anchors: BTreeSet<&str> = anchors.iter().map(String::as_str).collect();
    anchor_side(g, &anchors).map(|s| (s as u8, Source::Anchor))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub(crate) mod tests {
    use super::*;
    use crate::scouting::summary::{Player, PlayerStats};

    pub fn player(toon: &str, hero: &str) -> Player {
        Player {
            toon: toon.into(),
            name: toon.into(),
            hero: hero.into(),
            stats: PlayerStats::default(),
        }
    }

    /// Partie synthétique : `a` et `b` = toons des côtés 0 et 1 (héros = « H_<toon> »).
    pub fn game(a: &[&str], b: &[&str]) -> GameSummary {
        let side = |ts: &[&str]| ts.iter().map(|t| player(t, &format!("H_{t}"))).collect();
        GameSummary {
            date: Some("2026-07-22T19:21:46Z".into()),
            map: "Braxis Holdout".into(),
            build: Some(97605),
            length_s: 900.0,
            winner: Some(0),
            teams: [side(a), side(b)],
            bans: [Vec::new(), Vec::new()],
            picks: [Vec::new(), Vec::new()],
            first_pick: Some(0),
            first_fort: None,
            first_objective: None,
            level10: [None, None],
            level_diff: Vec::new(),
        }
    }

    const US: [&str; 5] = ["r1", "r2", "r3", "r4", "r5"];

    #[test]
    fn trois_du_roster_suffisent() {
        let g = game(&["x1", "r1", "x2", "r2", "r3"], &["o1", "o2", "o3", "o4", "o5"]);
        let roster: Vec<String> = US.iter().map(|s| s.to_string()).collect();
        assert_eq!(detect(&g, &roster, &[]), Some((0, Source::Roster)));
        let g = game(&["o1", "o2", "o3", "o4", "o5"], &US);
        assert_eq!(detect(&g, &roster, &[]), Some((1, Source::Roster)));
    }

    #[test]
    fn moins_de_trois_repli_sur_ancre_puis_rien() {
        let g = game(&["r1", "r2", "x1", "x2", "x3"], &["o1", "o2", "o3", "o4", "o5"]);
        let roster: Vec<String> = US.iter().map(|s| s.to_string()).collect();
        assert_eq!(detect(&g, &roster, &[]), None);
        assert_eq!(detect(&g, &roster, &["r2".into()]), Some((0, Source::Anchor)));
    }

    #[test]
    fn ancre_des_deux_cotes_ne_tranche_pas() {
        let g = game(&["a", "x1", "x2", "x3", "x4"], &["a", "o2", "o3", "o4", "o5"]);
        assert_eq!(detect(&g, &[], &["a".into()]), None);
    }

    #[test]
    fn candidats_a_cinquante_pour_cent() {
        let games = vec![
            game(&US, &["o1", "o2", "o3", "o4", "o5"]),
            game(&US, &["p1", "p2", "p3", "p4", "p5"]),
            game(&["r1", "r2", "r3", "r4", "sub"], &["q1", "q2", "q3", "q4", "q5"]),
        ];
        let d = candidates(&games, &[]);
        assert!(!d.ambiguous);
        let toons: Vec<&str> = d.candidates.iter().map(|c| c.toon.as_str()).collect();
        assert_eq!(toons.len(), 5, "{toons:?}"); // r1..r5, pas le remplaçant (1/3)
        assert!(!toons.contains(&"sub"));
        assert_eq!(effective_roster(&[], &d).len(), 5);
    }

    #[test]
    fn serie_contre_un_meme_adversaire_ambigue_sans_ancre() {
        let them = ["o1", "o2", "o3", "o4", "o5"];
        let games = vec![game(&US, &them), game(&them, &US), game(&US, &them)];
        let d = candidates(&games, &[]);
        assert!(d.ambiguous);
        assert_eq!(d.candidates.len(), 10);
        assert!(effective_roster(&[], &d).is_empty());
        // avec l'ancre : les 5 coéquipiers de l'ancre, et plus d'ambiguïté
        let d = candidates(&games, &["r1".into()]);
        assert!(!d.ambiguous);
        let mut toons: Vec<&str> = d.candidates.iter().map(|c| c.toon.as_str()).collect();
        toons.sort();
        assert_eq!(toons, US.to_vec());
    }

    #[test]
    fn roster_fixe_prioritaire() {
        let d = Detection { candidates: Vec::new(), ambiguous: true };
        assert_eq!(effective_roster(&["z".into()], &d), vec!["z".to_string()]);
    }
}
