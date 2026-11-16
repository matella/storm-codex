#![allow(clippy::expect_used)]

use std::fs;

use storm_codex_watcher::detector::{detect_replay_dirs_in, find_replay_dirs_under};

#[test]
fn detector_finds_nested_replay_dirs() {
    let docs = tempfile::tempdir().expect("tempdir");
    let accounts = docs.path().join("Heroes of the Storm").join("Accounts");
    let with_replays = accounts
        .join("1")
        .join("2-Hero-1-3")
        .join("Replays")
        .join("Multiplayer");
    let empty_toon = accounts
        .join("1")
        .join("2-Hero-1-4")
        .join("Replays")
        .join("Multiplayer");
    fs::create_dir_all(&with_replays).expect("mkdir");
    fs::create_dir_all(&empty_toon).expect("mkdir");
    fs::write(with_replays.join("2026-11-01 Game.StormReplay"), b"x").expect("write");
    fs::write(accounts.join("1").join("Variables.txt"), b"x").expect("write");

    // Install layout: every Multiplayer folder, even an empty one (it is watched for new games).
    let mut detected = detect_replay_dirs_in(docs.path());
    detected.sort();
    let mut expected = vec![with_replays.clone(), empty_toon];
    expected.sort();
    assert_eq!(detected, expected);

    // Parent scan: only folders that actually contain replays, at any depth.
    assert_eq!(find_replay_dirs_under(docs.path()), vec![with_replays]);
}

#[test]
fn detector_returns_empty_when_no_install() {
    let docs = tempfile::tempdir().expect("tempdir");
    assert!(detect_replay_dirs_in(docs.path()).is_empty());
    assert!(find_replay_dirs_under(docs.path()).is_empty());

    let missing = docs.path().join("does-not-exist");
    assert!(detect_replay_dirs_in(&missing).is_empty());
    assert!(find_replay_dirs_under(&missing).is_empty());
}
