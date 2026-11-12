#![allow(clippy::expect_used)]

use std::fs;
use std::time::Instant;

use storm_codex_watcher::ledger::{Ledger, Outcome};

#[test]
fn deferred_records_are_not_on_disk_until_flush() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("uploaded.json");

    let mut ledger = Ledger::open(&path).expect("open");
    ledger.record_deferred("a.StormReplay", Outcome::Uploaded);
    ledger.record_deferred("b.StormReplay", Outcome::Rejected("bad".into()));
    assert!(
        ledger.contains("a.StormReplay"),
        "visible in memory at once"
    );
    assert!(ledger.is_dirty());
    assert!(!path.exists(), "nothing written before flush");
    assert!(!Ledger::open(&path).expect("open").contains("a.StormReplay"));

    ledger.flush().expect("flush");
    assert!(!ledger.is_dirty());
    let reopened = Ledger::open(&path).expect("reopen");
    assert!(reopened.contains("a.StormReplay"));
    assert_eq!(
        reopened.rejected(),
        vec![("b.StormReplay".to_string(), "bad".to_string())]
    );

    // A clean ledger does not rewrite the file.
    fs::remove_file(&path).expect("remove");
    ledger.flush().expect("flush clean");
    assert!(!path.exists());

    // Compact JSON (one line).
    ledger
        .record("c.StormReplay", Outcome::Duplicate)
        .expect("record");
    let text = fs::read_to_string(&path).expect("read");
    assert!(!text.contains('\n'), "compact JSON: {text}");
}

/// Measurement, not a gate (3,000 fsync'd writes take a while): run with
/// `cargo test -p storm-codex-watcher --test ledger -- --ignored --nocapture`.
#[test]
#[ignore = "timing measurement"]
fn ledger_deferred_vs_write_through_timing() {
    const N: usize = 3_000;
    let dir = tempfile::tempdir().expect("tempdir");
    let name = |i: usize| format!("2026-11-12 12.{i:05} Cursed Hollow.StormReplay");

    let mut through = Ledger::open(&dir.path().join("through.json")).expect("open");
    let t0 = Instant::now();
    for i in 0..N {
        through.record(&name(i), Outcome::Uploaded).expect("record");
    }
    let write_through = t0.elapsed();

    let mut deferred = Ledger::open(&dir.path().join("deferred.json")).expect("open");
    let t0 = Instant::now();
    for i in 0..N {
        deferred.record_deferred(&name(i), Outcome::Uploaded);
        if (i + 1) % 250 == 0 {
            deferred.flush().expect("flush");
        }
    }
    deferred.flush().expect("flush");
    let batched = t0.elapsed();

    println!("{N} write-through records: {write_through:?}");
    println!("{N} deferred records + flush every 250: {batched:?}");
}

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
