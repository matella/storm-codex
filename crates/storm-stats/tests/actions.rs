//! storm-codex extension: `Output::actions` (number of `SCmdEvent` commands per toon, game in progress).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

#[test]
fn commandes_comptees_par_toon_et_plausibles() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../storm-replay/tests/data");
    let Some(replay) = std::fs::read_dir(&dir).ok().and_then(|d| {
        d.filter_map(Result::ok)
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|x| x == "StormReplay"))
    }) else {
        return;
    };
    let out = storm_stats::process_replay(&replay, "x.StormReplay");
    let Some(players) = out.players.as_ref() else {
        return;
    };
    assert!(!out.actions.is_empty(), "at least one player counted");
    let length = out
        .match_
        .as_ref()
        .and_then(|m| m.get("length"))
        .and_then(serde_json::Value::as_f64)
        .expect("game duration");
    for (toon, n) in &out.actions {
        assert!(
            players.contains_key(toon),
            "count attached to a known player"
        );
        let apm = *n as f64 / (length / 60.0);
        assert!(
            (5.0..=600.0).contains(&apm),
            "plausible APM, saw {apm} for {toon}"
        );
    }
}
