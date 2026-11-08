//! Extension storm-codex : `Output::actions` (nombre de commandes `SCmdEvent` par toon, partie en cours).
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
    assert!(!out.actions.is_empty(), "au moins un joueur compté");
    let length = out
        .match_
        .as_ref()
        .and_then(|m| m.get("length"))
        .and_then(serde_json::Value::as_f64)
        .expect("durée de partie");
    for (toon, n) in &out.actions {
        assert!(
            players.contains_key(toon),
            "compte rattaché à un joueur connu"
        );
        let apm = *n as f64 / (length / 60.0);
        assert!(
            (5.0..=600.0).contains(&apm),
            "APM plausible, vu {apm} pour {toon}"
        );
    }
}
