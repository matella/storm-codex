//! Smoke tests on the local corpus (corpus/spike50, not committed).
//! The binary is tested through its modules: the sources are recompiled as a light integration.

use std::path::PathBuf;
use std::process::Command;

fn corpus_dir() -> PathBuf {
    std::env::var_os("SPIKE_CORPUS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/spike50")
        })
}

/// Most recent replay (names start with the date → lexical sort is enough).
fn newest_replay() -> PathBuf {
    let mut files: Vec<PathBuf> = std::fs::read_dir(corpus_dir())
        .expect("corpus/spike50 absent — lancer spike/sample_corpus.ps1")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "StormReplay"))
        .collect();
    files.sort();
    files.pop().expect("corpus vide")
}

fn run_summary(path: &PathBuf) -> serde_json::Value {
    let exe = env!("CARGO_BIN_EXE_storm-decode");
    let out = Command::new(exe).arg(path).output().expect("running storm-decode");
    assert!(
        out.status.success(),
        "storm-decode failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("invalid JSON output")
}

#[test]
fn header_smoke() {
    let s = run_summary(&newest_replay());
    let signature = s["signature"].as_str().expect("signature");
    assert!(
        signature.starts_with("Heroes of the Storm replay"),
        "unexpected signature: {signature:?}"
    );
    assert!(s["base_build"].as_u64().expect("base_build") > 90_000);
    assert!(s["elapsed_game_loops"].as_i64().expect("loops") > 0);
}

#[test]
fn details_smoke() {
    let s = run_summary(&newest_replay());
    assert!(!s["map"].as_str().expect("map").is_empty(), "empty map");
    let players = s["players"].as_array().expect("players");
    assert_eq!(players.len(), 10, "expected 10 players");
    for p in players {
        assert!(!p["name"].as_str().expect("name").is_empty(), "nom vide");
        assert!(!p["hero"].as_str().expect("hero").is_empty(), "empty hero");
    }
    let winners = players.iter().filter(|p| p["result"] == 1).count();
    assert_eq!(winners, 5, "attendu 5 vainqueurs");
}

#[test]
fn tracker_smoke() {
    let s = run_summary(&newest_replay());
    assert!(
        s["tracker_events"].as_u64().expect("tracker_events") > 1_000,
        "too few events: {}",
        s["tracker_events"]
    );
    assert!(s["stats_events"].as_u64().expect("stats_events") > 0, "no SStatGameEvent");
    assert!(s["score_events"].as_u64().expect("score_events") > 0, "no SScoreResultEvent");
}
