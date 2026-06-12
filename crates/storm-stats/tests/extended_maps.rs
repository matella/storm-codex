//! Extension storm-stats : les cartes ARAM récentes (absentes de hots-parser 7.55.7) doivent
//! parser avec des invariants structurels sains. Pas de baseline Node (la référence les rejette)
//! → on valide la structure, pas une parité champ par champ. Voir EXTRA_MAPS dans process.rs.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;

#[test]
fn aram_map_parses_with_sane_structure() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/silver-city-aram.StormReplay");
    let out = storm_stats::process_replay(&path, "silver-city-aram.StormReplay");
    assert_eq!(out.status, 1, "carte ARAM rejetée (statut {})", out.status);
    let json = out.to_json();

    let m = &json["match"];
    assert_eq!(m["map"], "Silver City", "carte mal résolue");
    // ARAM = pas d'objectif PvE : objectif minimal {type}, firstObjective null
    assert_eq!(m["objective"], serde_json::json!({"type": "Silver City"}));
    assert!(
        m["firstObjective"].is_null(),
        "ARAM ne doit pas avoir de firstObjective"
    );
    assert!(
        m["takedowns"].as_array().is_some_and(|a| !a.is_empty()),
        "takedowns absents"
    );
    assert!(
        m["teams"]["0"]["stats"].is_object(),
        "stats d'équipe absentes"
    );

    let players = m["winner"].as_i64();
    assert!(
        players == Some(0) || players == Some(1),
        "vainqueur invalide"
    );

    let ps = json["players"].as_object().expect("players");
    assert_eq!(ps.len(), 10, "attendu 10 joueurs");
    let wins = ps.values().filter(|p| p["win"] == true).count();
    assert_eq!(wins, 5, "attendu 5 vainqueurs");
    for p in ps.values() {
        assert!(!p["hero"].as_str().unwrap_or("").is_empty(), "héros vide");
        // le score screen complet doit être présent (chemin universel)
        assert!(p["gameStats"]["Takedowns"].is_number(), "gameStats absent");
    }
}
