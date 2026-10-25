//! Extraction du draft d'une partie Custom depuis la sortie de storm-stats : reconstruit l'ordre
//! global des 16 étapes (miroir de `selections` dans `process_draft`, storm-stats/src/process.rs).
use super::replay::LoadedGame;
use super::Side;
use serde_json::{Map, Value as J};
use std::collections::HashSet;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ReplayError {
    #[error("partie non personnalisée : seules les parties Custom sont acceptées")]
    NotCustom,
    #[error("aucun draft exploitable dans ce replay")]
    NoDraft,
    #[error("replay illisible (statut storm-stats {0})")]
    Parse(i64),
}

#[derive(Clone, Copy)]
enum Slot {
    Ban(usize),
    Pick(usize),
}

/// Build à partir duquel les bans de 1re phase sont 2 par équipe + 1 en 2e phase (cf. process.rs).
const NEW_BAN_BUILD: f64 = 66292.0;

/// (équipe, créneau) pour chaque étape ; `a` = équipe first-pick, `b` = l'autre.
fn sequence(a: usize, b: usize, old_build: bool) -> [(usize, Slot); 16] {
    use Slot::{Ban, Pick};
    [
        (a, Ban(0)),
        (b, Ban(0)),
        (a, Ban(1)),
        (b, Ban(1)),
        (a, Pick(0)),
        (b, Pick(0)),
        (b, Pick(1)),
        (a, Pick(1)),
        (a, Pick(2)),
        (b, Ban(if old_build { 1 } else { 2 })),
        (a, Ban(if old_build { 1 } else { 2 })),
        (b, Pick(2)),
        (b, Pick(3)),
        (a, Pick(3)),
        (a, Pick(4)),
        (b, Pick(4)),
    ]
}

fn team_list(v: Option<&J>, team: usize) -> Vec<J> {
    v.and_then(|o| o.get(team.to_string())).and_then(J::as_array).cloned().unwrap_or_default()
}

/// Construit le `LoadedGame` d'une sortie storm-stats. `attrs` : code attribut des bans → nom canonique.
pub fn loaded_game_from_output(
    out: &storm_stats::Output,
    source: &str,
    attrs: &J,
) -> Result<LoadedGame, ReplayError> {
    if out.status != 1 {
        return Err(ReplayError::Parse(out.status));
    }
    let (Some(m), Some(players)) = (out.match_.as_ref(), out.players.as_ref()) else {
        return Err(ReplayError::Parse(out.status));
    };
    if m.get("mode").and_then(J::as_i64) != Some(storm_stats::constants::rt_int("GameMode", "Custom")) {
        return Err(ReplayError::NotCustom);
    }
    let (bans, picks) = (m.get("bans"), m.get("picks"));
    let picks_t = [team_list(picks, 0), team_list(picks, 1)];
    if bans.is_none() || picks.is_none() || picks_t.iter().all(Vec::is_empty) {
        return Err(ReplayError::NoDraft);
    }
    let bans_t = [team_list(bans, 0), team_list(bans, 1)];

    // Même défaut que le parser : sans `picks.first`, l'équipe 1 draft en premier.
    let a = if picks.and_then(|p| p.get("first")).and_then(J::as_i64) == Some(0) { 0 } else { 1 };
    let b = 1 - a;
    let build = m.get("version").and_then(|v| v.get("m_build")).and_then(J::as_f64).unwrap_or(f64::NAN);
    let old_build = build < NEW_BAN_BUILD;

    let mut script: Vec<Option<String>> = Vec::with_capacity(16);
    let mut pick_players: Vec<Option<String>> = Vec::with_capacity(16);
    let mut used: HashSet<String> = HashSet::new();
    let (mut blue_players, mut red_players) = (Vec::new(), Vec::new());
    for (i, (team, slot)) in sequence(a, b, old_build).into_iter().enumerate() {
        let (hero, player) = match slot {
            Slot::Ban(k) => {
                // Builds anciens : les bans 3 et 4 n'existent pas (« N/A » côté parser) → étape vide.
                let skipped = old_build && (i == 2 || i == 3);
                let code = (!skipped).then(|| bans_t[team].get(k)).flatten();
                let code = code.and_then(|b| b.get("hero").or(Some(b))).and_then(J::as_str);
                (code.and_then(|c| attrs.get(c)).and_then(J::as_str).map(str::to_owned), None)
            }
            Slot::Pick(k) => {
                let hero = picks_t[team].get(k).and_then(J::as_str).map(str::to_owned);
                let player = hero.as_deref().and_then(|h| owner(players, team, h, &mut used));
                (hero, player)
            }
        };
        if matches!(slot, Slot::Pick(_)) {
            let side_is_blue = team == 0;
            let list = if side_is_blue { &mut blue_players } else { &mut red_players };
            list.push(player.clone().unwrap_or_default());
        }
        script.push(hero);
        pick_players.push(player);
    }
    Ok(LoadedGame {
        source: source.to_owned(),
        map: m.get("map").and_then(J::as_str).unwrap_or("").to_owned(),
        first_pick: if a == 0 { Side::Blue } else { Side::Red },
        script,
        pick_players,
        blue_players: pad5(blue_players),
        red_players: pad5(red_players),
    })
}

/// Pseudo du joueur de l'équipe `team` qui joue `hero` (premier non encore attribué).
fn owner(players: &Map<String, J>, team: usize, hero: &str, used: &mut HashSet<String>) -> Option<String> {
    let (toon, p) = players.iter().find(|(toon, p)| {
        p.get("team").and_then(J::as_i64) == Some(team as i64)
            && p.get("hero").and_then(J::as_str) == Some(hero)
            && !used.contains(*toon)
    })?;
    used.insert(toon.clone());
    p.get("name").and_then(J::as_str).map(str::to_owned)
}

fn pad5(mut v: Vec<String>) -> [String; 5] {
    v.resize(5, String::new());
    std::array::from_fn(|i| std::mem::take(&mut v[i]))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use serde_json::json;

    fn codes(n: usize) -> Vec<(String, String)> {
        storm_stats::constants::hero_attribute()
            .as_object()
            .unwrap()
            .iter()
            .filter_map(|(c, h)| h.as_str().map(|h| (c.clone(), h.to_owned())))
            .take(n)
            .collect()
    }

    /// Sortie synthétique : 5 bans × 2 équipes (codes réels), picks `T<t>H<k>`, joueurs `P<t><k>`.
    fn output(mode: &str, first: i64, build: i64) -> storm_stats::Output {
        let c = codes(6);
        let ban = |i: usize| json!({"hero": c[i].0, "order": 1, "absolute": 1});
        let mut m = Map::new();
        m.insert("mode".into(), json!(storm_stats::constants::rt_int("GameMode", mode)));
        m.insert("map".into(), json!("Sky Temple"));
        m.insert("version".into(), json!({"m_build": build}));
        m.insert("bans".into(), json!({"0": [ban(0), ban(1), ban(2)], "1": [ban(3), ban(4), ban(5)]}));
        let pk = |t: usize| (0..5).map(|k| json!(format!("T{t}H{k}"))).collect::<Vec<_>>();
        m.insert("picks".into(), json!({"0": pk(0), "1": pk(1), "first": first}));
        let mut players = Map::new();
        for t in 0..2usize {
            for k in 0..5usize {
                players.insert(format!("toon{t}{k}"), json!({"team": t, "hero": format!("T{t}H{k}"), "name": format!("P{t}{k}")}));
            }
        }
        storm_stats::Output { status: 1, match_: Some(m), players: Some(players) }
    }

    fn load(o: &storm_stats::Output) -> Result<LoadedGame, ReplayError> {
        loaded_game_from_output(o, "g.StormReplay", storm_stats::constants::hero_attribute())
    }

    #[test]
    fn script_follows_standard_order_team0_first() {
        let g = load(&output("Custom", 0, 90000)).unwrap();
        let want = |i: usize| g.script[i].as_deref().unwrap();
        assert_eq!(want(4), "T0H0");
        assert_eq!((want(5), want(6)), ("T1H0", "T1H1"));
        assert_eq!((want(7), want(8)), ("T0H1", "T0H2"));
        assert_eq!((want(11), want(12)), ("T1H2", "T1H3"));
        assert_eq!((want(13), want(14)), ("T0H3", "T0H4"));
        assert_eq!(want(15), "T1H4");
        assert_eq!(g.first_pick, Side::Blue);
    }

    #[test]
    fn team1_first_pick_makes_red_first() {
        let g = load(&output("Custom", 1, 90000)).unwrap();
        assert_eq!(g.script[4].as_deref(), Some("T1H0"));
        assert_eq!(g.first_pick, Side::Red);
    }

    #[test]
    fn bans_resolved_to_canonical_names() {
        let g = load(&output("Custom", 0, 90000)).unwrap();
        let c = codes(6);
        // ordre des bans : a0 b0 a1 b1 | b2 a2
        let expect = [(0, &c[0]), (1, &c[3]), (2, &c[1]), (3, &c[4]), (9, &c[5]), (10, &c[2])];
        for (i, (_, hero)) in expect {
            assert_eq!(g.script[i].as_deref(), Some(hero.as_str()), "étape {i}");
        }
    }

    #[test]
    fn non_custom_rejected() {
        assert_eq!(load(&output("Brawl", 0, 90000)), Err(ReplayError::NotCustom));
    }

    #[test]
    fn old_build_has_none_steps() {
        let g = load(&output("Custom", 0, 66000)).unwrap();
        assert!(g.script[2].is_none() && g.script[3].is_none());
        assert!(g.script[0].is_some() && g.script[1].is_some());
        assert!(g.script[9].is_some() && g.script[10].is_some()); // bans « 2e phase » = index 1
    }

    #[test]
    fn status_not_one_is_parse_error() {
        let mut o = output("Custom", 0, 90000);
        o.status = -3;
        assert_eq!(load(&o), Err(ReplayError::Parse(-3)));
    }

    #[test]
    fn pick_players_follow_pick_owner() {
        let g = load(&output("Custom", 0, 90000)).unwrap();
        assert_eq!(g.pick_players[4].as_deref(), Some("P00"));
        assert_eq!(g.pick_players[6].as_deref(), Some("P11"));
        assert_eq!(g.blue_players[0], "P00");
        assert_eq!(g.blue_players[4], "P04");
        assert_eq!(g.red_players[3], "P13");
        assert!(g.pick_players[0].is_none());
    }

    #[test]
    fn missing_bans_or_picks_is_no_draft() {
        let mut o = output("Custom", 0, 90000);
        o.match_.as_mut().unwrap().remove("picks");
        assert_eq!(load(&o), Err(ReplayError::NoDraft));
    }

    /// Fixture réelle (Custom, build 98025, pseudos anonymisés) : le script reconstruit doit placer
    /// chaque héros à l'étape `turn` calculée par le parser (storm-stats `process_draft`).
    #[test]
    fn real_custom_fixture_matches_parser_turns() {
        let fx: J = serde_json::from_str(include_str!("fixtures/custom_draft.json")).unwrap();
        let out = storm_stats::Output {
            status: 1,
            match_: fx["match"].as_object().cloned(),
            players: fx["players"].as_object().cloned(),
        };
        let g = load(&out).unwrap();
        assert_eq!(g.first_pick, Side::Red); // picks.first = 1 → équipe 1 (Red) draft en premier
        for p in fx["players"].as_object().unwrap().values() {
            let (hero, turn) = (p["hero"].as_str().unwrap(), p["turn"].as_i64().unwrap());
            assert_eq!(g.script.iter().position(|h| h.as_deref() == Some(hero)), Some(turn as usize), "{hero}");
        }
        assert_eq!(g.script.iter().flatten().count(), 16);
    }
}
