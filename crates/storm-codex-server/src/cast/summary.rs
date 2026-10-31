//! Résumé de partie (post-game breakdown) depuis la sortie storm-stats d'un replay Custom.
//! Chemins JSON lus : guide de lecture de docs/spec/03-storm-stats.md (« Résumé de cast »).
//! Une valeur absente donne `None` / vide — jamais une valeur inventée.
use super::{GameSummary, PlayerSummary};
use crate::draft::Side;
use serde_json::{Map, Value as J};
use std::collections::BTreeMap;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SummaryError {
    #[error("partie non personnalisée : seules les parties Custom sont acceptées")]
    NotCustom,
    #[error("replay illisible (statut storm-stats {0})")]
    Parse(i64),
    #[error("valeur absente de la sortie storm-stats : {0}")]
    Missing(&'static str),
}

/// Équipe storm-stats (0/1) → côté de la série (0 = Blue, comme le draft depuis replay).
fn side_of(team: i64) -> Option<Side> {
    match team {
        0 => Some(Side::Blue),
        1 => Some(Side::Red),
        _ => None,
    }
}

/// Entier positif d'un nombre JSON (entier ou flottant ; storm-stats écrit parfois `28.0`).
fn uint(v: Option<&J>) -> Option<u64> {
    let v = v?;
    v.as_u64().or_else(|| v.as_f64().filter(|f| f.is_finite() && *f >= 0.0).map(|f| f as u64))
}

fn u32_of(v: Option<&J>) -> Option<u32> {
    uint(v).map(|n| u32::try_from(n).unwrap_or(u32::MAX))
}

/// `f(équipe)` pour les deux équipes ; `None` dès qu'une manque.
fn both(f: impl Fn(&str) -> Option<u32>) -> Option<[u32; 2]> {
    Some([f("0")?, f("1")?])
}

/// Forts + keeps détruits par l'équipe : `teams[t].stats.structures.{Fort,Keep}.destroyed`
/// (bâtiments seuls, sans tours ni puits). Carte sans aucune sous-clé `Fort`/`Keep` (dans aucune
/// équipe) → `None` (ligne retirée, pas de 0–0 inventé) ; sinon une sous-clé absente compte 0.
fn forts_keeps(m: &Map<String, J>) -> Option<[u32; 2]> {
    let st = |t: &str| m.get("teams")?.get(t)?.get("stats")?.get("structures")?.as_object();
    let has = |t: &str| st(t).is_some_and(|o| o.contains_key("Fort") || o.contains_key("Keep"));
    if !has("0") && !has("1") {
        return None;
    }
    both(|t| {
        let st = st(t)?;
        let n = |k: &str| u32_of(st.get(k).and_then(|s| s.get("destroyed"))).unwrap_or(0);
        Some(n("Fort") + n("Keep"))
    })
}

/// Cartes (clés `MapType` de storm-stats) dont `objective[t].events` sont des objectifs remportés,
/// vérifié sur l'événement qui les produit (process.rs) et les comptes du corpus Custom :
/// TowersOfDoom = `AltarCaptured` ; CursedHollow = `TributeCollected` ; DragonShire =
/// `DragonKnightActivated` ; HauntedWoods = `GardenTerrorActivated` ; Crypts = `SpidersSpawned` ;
/// Volskaya = naissance du `Triglav` capturé ; BlackheartsBay = `GhostShipCaptured`.
/// Toute autre carte → `None` (ligne retirée) : guide de lecture de docs/spec/03-storm-stats.md.
const OBJECTIVE_EVENT_MAPS: [&str; 7] =
    ["TowersOfDoom", "CursedHollow", "DragonShire", "HauntedWoods", "Crypts", "Volskaya", "BlackheartsBay"];
/// Battlefield of Eternity : `objective.results[].winner` (`ImmortalDefeated`).
const OBJECTIVE_RESULTS_MAP: &str = "BattlefieldOfEternity";

fn is_map(m: &Map<String, J>, key: &str) -> bool {
    m.get("map").and_then(J::as_str) == Some(storm_stats::constants::rt_str("MapType", key))
}

/// Objectifs de carte par équipe, cartes de la liste blanche seulement ; comme `objectiveEvents`
/// de la fiche, un événement sans `time` n'est pas compté.
fn objectives(m: &Map<String, J>) -> Option<[u32; 2]> {
    let o = m.get("objective")?.as_object()?;
    let timed = |e: &&J| e.get("time").is_some_and(|t| !t.is_null());
    let mut n = [0u32; 2];
    if is_map(m, OBJECTIVE_RESULTS_MAP) {
        for r in o.get("results")?.as_array()?.iter().filter(timed) {
            if let Some(i) = r.get("winner").and_then(J::as_f64).and_then(|w| side_of(w as i64)) {
                n[i as usize] += 1;
            }
        }
        return Some(n);
    }
    if !OBJECTIVE_EVENT_MAPS.iter().any(|k| is_map(m, k)) {
        return None;
    }
    for (i, t) in ["0", "1"].into_iter().enumerate() {
        let ev = o.get(t)?.get("events")?.as_array()?;
        n[i] = u32::try_from(ev.iter().filter(timed).count()).unwrap_or(u32::MAX);
    }
    Some(n)
}

impl GameSummary {
    /// Même partie vue de l'autre côté : équipe 0 du replay = Red. Paires et bans échangés,
    /// vainqueur et côtés des joueurs inversés, avance d'XP négée, joueurs re-triés (Blue d'abord,
    /// ordre de pick conservé).
    pub fn swapped(mut self) -> GameSummary {
        let sw = |p: &mut [u32; 2]| p.swap(0, 1);
        self.winner = self.winner.other();
        sw(&mut self.takedowns);
        for p in [&mut self.team_level, &mut self.structures, &mut self.merc_camps, &mut self.objectives].into_iter().flatten() {
            sw(p);
        }
        self.bans.swap(0, 1);
        for p in &mut self.players {
            p.side = p.side.other();
        }
        self.players.sort_by_key(|p| p.side == Side::Red);
        for x in &mut self.xp_lead {
            x.1 = x.1.saturating_neg();
        }
        self
    }
}

/// Avance d'XP de Blue par échantillon de `XPBreakdown` : somme des champs numériques de
/// `breakdown` hors champs de temps (même calcul que la courbe « Team XP » de la fiche),
/// équipes appariées par `loop`.
fn xp_lead(m: &Map<String, J>) -> Vec<(u32, i32)> {
    let Some(rows) = m.get("XPBreakdown").and_then(J::as_array) else { return Vec::new() };
    let mut by_loop: BTreeMap<i64, (f64, [Option<f64>; 2])> = BTreeMap::new();
    for r in rows {
        let (Some(lp), Some(team), Some(b)) = (
            r.get("loop").and_then(J::as_f64),
            r.get("team").and_then(J::as_f64).and_then(|t| side_of(t as i64)),
            r.get("breakdown").and_then(J::as_object),
        ) else {
            continue;
        };
        let xp: f64 = b.iter().filter(|(k, _)| !k.to_ascii_lowercase().contains("time")).filter_map(|(_, v)| v.as_f64()).sum();
        let e = by_loop.entry(lp as i64).or_insert((0.0, [None, None]));
        e.0 = r.get("time").and_then(J::as_f64).unwrap_or(0.0);
        e.1[team as usize] = Some(xp);
    }
    by_loop
        .into_values()
        .filter_map(|(t, [b, r])| Some((t.max(0.0) as u32, (b? - r?).round() as i32)))
        .collect()
}

fn bans(m: &Map<String, J>) -> [Vec<String>; 2] {
    let attrs = storm_stats::constants::hero_attribute();
    let team = |t: &str| -> Vec<String> {
        m.get("bans")
            .and_then(|b| b.get(t))
            .and_then(J::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|b| b.get("hero").or(Some(b)).and_then(J::as_str))
                    .filter_map(|code| attrs.get(code).and_then(J::as_str).map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    [team("0"), team("1")]
}

fn player(toon: &str, p: &J) -> Option<PlayerSummary> {
    let side = side_of(p.get("team")?.as_i64()?)?;
    let g = p.get("gameStats")?;
    let n32 = |k: &str| u32_of(g.get(k)).unwrap_or(0);
    let n64 = |k: &str| uint(g.get(k)).unwrap_or(0);
    Some(PlayerSummary {
        side,
        toon: toon.to_owned(),
        name: p.get("name").and_then(J::as_str).unwrap_or("").to_owned(),
        hero: p.get("hero").and_then(J::as_str).unwrap_or("").to_owned(),
        kills: n32("SoloKill"),
        takedowns: n32("Takedowns"),
        deaths: n32("Deaths"),
        assists: n32("Assists"),
        hero_damage: n64("HeroDamage"),
        siege_damage: n64("SiegeDamage"),
        healing: n64("Healing"),
    })
}

/// Joueurs Blue puis Red, dans l'ordre de pick de `match.picks[t]` (héros absent des picks : à la fin).
fn players(m: &Map<String, J>, players: &Map<String, J>) -> Vec<PlayerSummary> {
    let mut v: Vec<PlayerSummary> = players.iter().filter_map(|(toon, p)| player(toon, p)).collect();
    let pick_pos = |p: &PlayerSummary| {
        let t = if p.side == Side::Blue { "0" } else { "1" };
        m.get("picks")
            .and_then(|pk| pk.get(t))
            .and_then(J::as_array)
            .and_then(|l| l.iter().position(|h| h.as_str() == Some(p.hero.as_str())))
            .unwrap_or(usize::MAX)
    };
    v.sort_by_key(|p| (p.side == Side::Red, pick_pos(p), p.toon.clone()));
    v
}

pub fn summary_from_output(out: &storm_stats::Output) -> Result<GameSummary, SummaryError> {
    if out.status != 1 {
        return Err(SummaryError::Parse(out.status));
    }
    let (Some(m), Some(ps)) = (out.match_.as_ref(), out.players.as_ref()) else {
        return Err(SummaryError::Parse(out.status));
    };
    if m.get("mode").and_then(J::as_i64) != Some(storm_stats::constants::rt_int("GameMode", "Custom")) {
        return Err(SummaryError::NotCustom);
    }
    let winner = m.get("winner").and_then(J::as_f64).and_then(|w| side_of(w as i64)).ok_or(SummaryError::Missing("winner"))?;
    let length = m.get("length").and_then(J::as_f64).filter(|l| l.is_finite() && *l >= 0.0).ok_or(SummaryError::Missing("length"))?;
    let takedowns = both(|t| u32_of(m.get(&format!("team{t}Takedowns")))).ok_or(SummaryError::Missing("teamNTakedowns"))?;
    let players = players(m, ps);
    if players.is_empty() {
        return Err(SummaryError::Missing("players"));
    }
    let team_stat = |k: &'static str| both(|t| u32_of(m.get("teams")?.get(t)?.get("stats")?.get(k)));
    Ok(GameSummary {
        map: m.get("map").and_then(J::as_str).map(str::to_owned),
        played_at: m.get("date").and_then(J::as_str).map(str::to_owned),
        duration_secs: length as u32,
        winner,
        takedowns,
        team_level: both(|t| u32_of(m.get("teams")?.get(t)?.get("level"))),
        structures: forts_keeps(m),
        merc_camps: team_stat("mercCaptures"),
        objectives: objectives(m),
        bans: bans(m),
        players,
        xp_lead: xp_lead(m),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> J {
        serde_json::from_str(include_str!("fixtures/custom_summary.json")).unwrap()
    }

    fn output(fx: &J) -> storm_stats::Output {
        storm_stats::Output { status: 1, match_: fx["match"].as_object().cloned(), players: fx["players"].as_object().cloned() }
    }

    /// Fixture = replay Custom réel (Tomb of the Spider Queen, 2026-09-27), réduit et anonymisé.
    /// Les littéraux ont été recoupés par un second chemin de la sortie complète : niveau =
    /// max(`levelTimes`), camps = `mercs.captures` par équipe, forts·keeps = `match.structures`
    /// (Fort/Keep détruits chez l'adversaire). Sur CE fixture, la somme des `SoloKill` d'une équipe
    /// égale `teamNTakedowns` (28 / 15) : vrai ici, pas un invariant général de storm-stats.
    #[test]
    fn summary_matches_match_sheet_values() {
        let s = summary_from_output(&output(&fixture())).unwrap();
        assert_eq!(s.winner, Side::Blue);
        assert_eq!(s.duration_secs, 1576);
        assert_eq!(s.takedowns, [28, 15]);
        assert_eq!(s.team_level, Some([25, 24]));
        assert_eq!(s.merc_camps, Some([12, 6]));
        assert_eq!(s.structures, Some([5, 4]));
        assert_eq!(s.objectives, Some([3, 3]));
        assert_eq!(s.bans, [vec!["Johanna", "Yrel", "Whitemane"], vec!["Falstad", "Gul'dan", "Tracer"]]);
        assert_eq!(s.players.len(), 10);
        let p = &s.players[0];
        assert_eq!(
            (p.side, p.toon.as_str(), p.name.as_str(), p.hero.as_str()),
            (Side::Blue, "1-Hero-1-1001", "Player1", "Hogger")
        );
        assert_eq!((p.kills, p.takedowns, p.deaths, p.assists), (6, 13, 6, 7));
        assert_eq!((p.hero_damage, p.siege_damage, p.healing), (47487, 119533, 0));
        let heal = s.players.iter().find(|p| p.hero == "Malfurion").unwrap();
        assert_eq!(heal.healing, 161589);
        // Blue puis Red, ordre de pick de chaque équipe
        let heroes: Vec<_> = s.players.iter().map(|p| (p.side, p.hero.as_str())).collect();
        assert_eq!(heroes[4], (Side::Blue, "Junkrat"));
        assert_eq!(heroes[5], (Side::Red, "Blaze"));
        assert_eq!(heroes[9], (Side::Red, "Greymane"));
        // K/D/A d'équipe = sommes des joueurs (ligne « Team total » de la fiche)
        let sum = |side: Side, f: fn(&PlayerSummary) -> u32| s.players.iter().filter(|p| p.side == side).map(f).sum::<u32>();
        assert_eq!((sum(Side::Blue, |p| p.kills), sum(Side::Blue, |p| p.deaths), sum(Side::Blue, |p| p.assists)), (28, 15, 87));
        // avance d'XP : 27 échantillons appariés par `loop`, (s, Blue − Red)
        assert_eq!(s.xp_lead.len(), 27);
        assert_eq!(s.xp_lead[0], (60, 710));
        assert_eq!(s.xp_lead[26], (1583, 13120));
    }

    #[test]
    fn summary_rejects_non_custom() {
        let mut fx = fixture();
        fx["match"]["mode"] = json!(storm_stats::constants::rt_int("GameMode", "StormLeague"));
        assert_eq!(summary_from_output(&output(&fx)), Err(SummaryError::NotCustom));
    }

    #[test]
    fn summary_rejects_failed_status() {
        let mut o = output(&fixture());
        o.status = -2;
        assert_eq!(summary_from_output(&o), Err(SummaryError::Parse(-2)));
        let empty = storm_stats::Output { status: 1, match_: None, players: None };
        assert_eq!(summary_from_output(&empty), Err(SummaryError::Parse(1)));
    }

    #[test]
    fn missing_required_values_are_errors() {
        for key in ["winner", "length", "team0Takedowns"] {
            let mut fx = fixture();
            fx["match"].as_object_mut().unwrap().remove(key);
            assert!(matches!(summary_from_output(&output(&fx)), Err(SummaryError::Missing(_))), "{key}");
        }
        let mut fx = fixture();
        fx["match"]["winner"] = json!(-1);
        assert_eq!(summary_from_output(&output(&fx)), Err(SummaryError::Missing("winner")));
    }

    #[test]
    fn absent_optional_values_are_none_not_zero() {
        let mut fx = fixture();
        let m = fx["match"].as_object_mut().unwrap();
        m.remove("objective");
        m.remove("XPBreakdown");
        m.remove("bans");
        for t in ["0", "1"] {
            m["teams"][t]["stats"].as_object_mut().unwrap().remove("structures");
            m["teams"][t]["stats"].as_object_mut().unwrap().remove("mercCaptures");
        }
        let s = summary_from_output(&output(&fx)).unwrap();
        assert_eq!((s.structures, s.merc_camps, s.objectives), (None, None, None));
        assert!(s.xp_lead.is_empty());
        assert_eq!(s.bans, [Vec::<String>::new(), Vec::new()]);
        assert_eq!(s.team_level, Some([25, 24]));
    }

    /// Item 9 : carte sans Fort ni Keep (dans aucune équipe) → pas de ligne forts·keeps, jamais 0–0 ;
    /// une équipe sans sous-clé quand l'autre en a → 0 pour elle.
    #[test]
    fn forts_keeps_absent_on_maps_without_them() {
        let mut fx = fixture();
        for t in ["0", "1"] {
            let st = fx["match"]["teams"][t]["stats"]["structures"].as_object_mut().unwrap();
            st.remove("Fort");
            st.remove("Keep");
        }
        assert_eq!(summary_from_output(&output(&fx)).unwrap().structures, None);
        fx["match"]["teams"]["1"]["stats"]["structures"]["Keep"] = json!({"lost": 0, "destroyed": 1});
        assert_eq!(summary_from_output(&output(&fx)).unwrap().structures, Some([0, 1]));
    }

    /// R17 : liste blanche de cartes (`MapType`) ; événements sans `time` non comptés.
    #[test]
    fn objectives_follow_allowlist() {
        let mut fx = fixture();
        let set = |fx: &mut J, map: &str, obj: J| {
            fx["match"]["map"] = json!(map);
            fx["match"]["objective"] = obj;
        };
        // carte de la liste (Tomb = Crypts) : événements datés seulement
        set(&mut fx, "Tomb of the Spider Queen", json!({"type": "Tomb of the Spider Queen",
            "0": {"count": 3.0, "events": [{"time": 1.0}, {"time": 2.0}, {"time": null}]}, "1": {"count": 0, "events": []}}));
        assert_eq!(summary_from_output(&output(&fx)).unwrap().objectives, Some([2, 0]));
        set(&mut fx, "Cursed Hollow", json!({"type": "Cursed Hollow", "0": {"count": 3.0, "events": [{"time": 1.0}, {"time": 2.0}, {"time": 3.0}]}, "1": {"count": 0, "events": []}}));
        assert_eq!(summary_from_output(&output(&fx)).unwrap().objectives, Some([3, 0]));
        set(&mut fx, "Battlefield of Eternity", json!({"type": "Battlefield of Eternity",
            "results": [{"winner": 1.0, "time": 272.0}, {"winner": 0.0, "time": 506.0}, {"winner": 1.0, "time": 766.0}]}));
        assert_eq!(summary_from_output(&output(&fx)).unwrap().objectives, Some([1, 2]));
        // hors liste → pas de ligne, même avec des événements par équipe
        for map in ["Sky Temple", "Alterac Pass", "Infernal Shrines", "Warhead Junction", "Haunted Mines", "Silver City", "Braxis Holdout"] {
            set(&mut fx, map, json!({"type": map, "0": {"count": 2.0, "events": [{"time": 1.0}, {"time": 2.0}]}, "1": {"count": 1.0, "events": [{"time": 3.0}]}}));
            assert_eq!(summary_from_output(&output(&fx)).unwrap().objectives, None, "{map}");
        }
    }

    #[test]
    fn swapped_mirrors_every_pair() {
        let s = summary_from_output(&output(&fixture())).unwrap();
        let w = s.clone().swapped();
        assert_eq!(w.winner, Side::Red);
        assert_eq!((w.takedowns, w.team_level, w.merc_camps, w.structures, w.objectives),
            ([15, 28], Some([24, 25]), Some([6, 12]), Some([4, 5]), Some([3, 3])));
        assert_eq!(w.bans[0], s.bans[1]);
        assert_eq!(w.xp_lead[0], (60, -710));
        assert_eq!((w.players[0].side, w.players[0].hero.as_str()), (Side::Blue, "Blaze"));
        assert_eq!((w.players[5].side, w.players[5].hero.as_str()), (Side::Red, "Hogger"));
        assert_eq!(w.swapped(), s);
    }
}
