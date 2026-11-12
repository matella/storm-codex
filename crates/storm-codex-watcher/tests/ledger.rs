#![allow(clippy::expect_used)]

use std::fs;

use storm_codex_watcher::ledger::{Ledger, Outcome};

#[test]
fn ledger_roundtrip_survives_reopen() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("state").join("uploaded.json");

    let mut ledger = Ledger::open(&path).expect("open new ledger");
    assert!(!ledger.contains("a.StormReplay"));
    ledger
        .record("a.StormReplay", Outcome::Uploaded)
        .expect("record");
    ledger
        .record("b.StormReplay", Outcome::Duplicate)
        .expect("record");
    ledger
        .record("c.StormReplay", Outcome::Rejected("not_custom".into()))
        .expect("record");
    drop(ledger);

    let reopened = Ledger::open(&path).expect("reopen");
    assert!(reopened.contains("a.StormReplay"));
    assert!(reopened.contains("b.StormReplay"));
    assert!(reopened.contains("c.StormReplay"));
    assert!(!reopened.contains("d.StormReplay"));
    assert_eq!(
        reopened.rejected(),
        vec![("c.StormReplay".to_string(), "not_custom".to_string())]
    );
}

#[test]
fn ledger_write_is_atomic_on_crash_simulation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("uploaded.json");

    let mut ledger = Ledger::open(&path).expect("open");
    ledger
        .record("a.StormReplay", Outcome::Uploaded)
        .expect("record");
    drop(ledger);

    // A crash between "write temp" and "rename" leaves a stale, half-written temp file.
    let tmp = dir.path().join("uploaded.json.tmp");
    fs::write(&tmp, b"{\"entries\": {\"half-writ").expect("write stale tmp");

    let mut reopened = Ledger::open(&path).expect("reopen despite stale tmp");
    assert!(reopened.contains("a.StormReplay"));
    reopened
        .record("b.StormReplay", Outcome::Uploaded)
        .expect("record over stale tmp");
    drop(reopened);

    let again = Ledger::open(&path).expect("reopen");
    assert!(again.contains("a.StormReplay"));
    assert!(again.contains("b.StormReplay"));
    // The committed file is always complete JSON.
    let text = fs::read_to_string(&path).expect("read ledger");
    serde_json::from_str::<serde_json::Value>(&text).expect("ledger is valid JSON");
}
