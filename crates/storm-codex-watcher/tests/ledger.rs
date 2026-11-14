#![allow(clippy::expect_used)]

use std::fs;
use std::time::{Duration, Instant};

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

#[test]
fn stale_temp_files_are_removed_on_open() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("uploaded.json");
    for stale in ["uploaded.json.tmp", "uploaded.json.4242.7.tmp"] {
        fs::write(dir.path().join(stale), b"{\"entries\": {\"half").expect("stale");
    }
    fs::write(dir.path().join("other.json.1.1.tmp"), b"x").expect("unrelated");
    Ledger::open(&path).expect("open");
    assert!(!dir.path().join("uploaded.json.tmp").exists());
    assert!(!dir.path().join("uploaded.json.4242.7.tmp").exists());
    assert!(
        dir.path().join("other.json.1.1.tmp").exists(),
        "only this ledger's temps are removed"
    );
}

/// An async flush cancelled mid-write keeps running on the blocking pool while a synchronous
/// flush (the backfill's drop guard) writes a newer snapshot: the file on disk must always be
/// valid JSON and end up with every record.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelled_async_flush_racing_a_sync_flush_never_corrupts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("uploaded.json");
    let mut ledger = Ledger::open(&path).expect("open");
    let mut expected = 0usize;
    for round in 0..200 {
        for i in 0..20 {
            ledger.record_deferred(&format!("r{round}-{i}.StormReplay"), Outcome::Uploaded);
            expected += 1;
        }
        // poll the async flush once (its blocking write starts), then drop it
        let _ = tokio::time::timeout(Duration::ZERO, ledger.flush_async()).await;
        ledger.record_deferred(&format!("r{round}-last.StormReplay"), Outcome::Duplicate);
        expected += 1;
        ledger.flush().expect("sync flush");
        let text = fs::read_to_string(&path).expect("read");
        assert!(
            serde_json::from_str::<serde_json::Value>(&text).is_ok(),
            "round {round}: ledger on disk is not valid JSON"
        );
    }
    // let the detached writes finish, then check the final state
    tokio::time::sleep(Duration::from_millis(200)).await;
    let text = fs::read_to_string(&path).expect("read");
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    assert_eq!(v["entries"].as_object().expect("entries").len(), expected);
}
