//! storm-codex extension: `Output::hotkey_profiles` (SUserOptionsEvent.m_hotkeyProfile per toon).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

#[test]
fn profils_de_raccourcis_extraits_par_toon() {
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
    assert!(!out.hotkey_profiles.is_empty(), "at least one profile read");
    for toon in out.hotkey_profiles.keys() {
        assert!(
            players.contains_key(toon),
            "profile attached to a known player"
        );
    }
}
