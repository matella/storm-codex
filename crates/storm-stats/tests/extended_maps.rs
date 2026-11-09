//! storm-stats extension: the recent ARAM maps (missing from hots-parser 7.55.7) must
//! parse with sound structural invariants. No Node baseline (the reference rejects them)
//! → we validate the structure, not a field-by-field parity. See EXTRA_MAPS in process.rs.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;

#[test]
fn aram_map_parses_with_sane_structure() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/silver-city-aram.StormReplay");
    let out = storm_stats::process_replay(&path, "silver-city-aram.StormReplay");
    assert_eq!(out.status, 1, "ARAM map rejected (status {})", out.status);
    let json = out.to_json();

    let m = &json["match"];
    assert_eq!(m["map"], "Silver City", "map wrongly resolved");
    // ARAM = no PvE objective: minimal objective {type}, firstObjective null
    assert_eq!(m["objective"], serde_json::json!({"type": "Silver City"}));
    assert!(
        m["firstObjective"].is_null(),
        "ARAM must not have a firstObjective"
    );
    assert!(
        m["takedowns"].as_array().is_some_and(|a| !a.is_empty()),
        "takedowns absents"
    );
    assert!(m["teams"]["0"]["stats"].is_object(), "team stats missing");

    let players = m["winner"].as_i64();
    assert!(
        players == Some(0) || players == Some(1),
        "vainqueur invalide"
    );

    let ps = json["players"].as_object().expect("players");
    assert_eq!(ps.len(), 10, "expected 10 players");
    let wins = ps.values().filter(|p| p["win"] == true).count();
    assert_eq!(wins, 5, "attendu 5 vainqueurs");
    for p in ps.values() {
        assert!(!p["hero"].as_str().unwrap_or("").is_empty(), "empty hero");
        // the full score screen must be present (universal path)
        assert!(p["gameStats"]["Takedowns"].is_number(), "gameStats absent");
    }
}
