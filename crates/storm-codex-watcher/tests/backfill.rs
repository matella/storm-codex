#![allow(clippy::expect_used)]

mod common;

use std::collections::BTreeSet;
use std::time::Duration;

use common::{spawn_server, write_replay, REJECT_BODY};
use storm_codex_watcher::backfill::{self, Progress};
use storm_codex_watcher::ledger::{Ledger, Outcome};
use storm_codex_watcher::upload::Uploader;

const FAST: Duration = Duration::from_millis(1);

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

#[tokio::test(flavor = "multi_thread")]
async fn backfill_flushes_ledger_at_end() {
    let server = spawn_server().await;
    let replays = tempfile::tempdir().expect("tempdir");
    let state = tempfile::tempdir().expect("tempdir");
    for i in 0..3u64 {
        write_replay(replays.path(), &format!("g{i}.StormReplay"), secs(600 + i));
    }
    let ledger_path = state.path().join("uploaded.json");
    let mut ledger = Ledger::open(&ledger_path).expect("ledger");
    let uploader = Uploader::new(server.base("mixed")).with_backoff_unit(FAST);
    let progress = Progress::default();
    backfill::run(
        &[replays.path().to_path_buf()],
        &uploader,
        &mut ledger,
        &progress,
    )
    .await;

    // Still holding the ledger: everything is already on disk.
    assert!(!ledger.is_dirty());
    let on_disk = Ledger::open(&ledger_path).expect("reopen");
    for i in 0..3 {
        assert!(on_disk.contains(&format!("g{i}.StormReplay")));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn backfill_flushes_ledger_when_cancelled() {
    let server = spawn_server().await;
    let replays = tempfile::tempdir().expect("tempdir");
    let state = tempfile::tempdir().expect("tempdir");
    write_replay(replays.path(), "good-1.StormReplay", secs(900));
    write_replay(replays.path(), "good-2.StormReplay", secs(800));
    // Answers 503: stuck in a 2 s backoff when the run is cancelled.
    write_replay(replays.path(), "slow.StormReplay", secs(700));

    let ledger_path = state.path().join("uploaded.json");
    let mut ledger = Ledger::open(&ledger_path).expect("ledger");
    let uploader = Uploader::new(server.base("mixed"));
    let progress = Progress::default();
    // Cancelled well before the 1 s periodic flush.
    let cancelled = tokio::time::timeout(
        Duration::from_millis(500),
        backfill::run(
            &[replays.path().to_path_buf()],
            &uploader,
            &mut ledger,
            &progress,
        ),
    )
    .await;
    assert!(
        cancelled.is_err(),
        "run should still be retrying slow.StormReplay"
    );
    assert_eq!(progress.uploaded(), 2);

    let on_disk = Ledger::open(&ledger_path).expect("reopen");
    assert!(on_disk.contains("good-1.StormReplay"));
    assert!(on_disk.contains("good-2.StormReplay"));
    assert!(!on_disk.contains("slow.StormReplay"));
}

#[tokio::test(flavor = "multi_thread")]
async fn backfill_skips_already_recorded_and_counts_rejected() {
    let server = spawn_server().await;
    let replays = tempfile::tempdir().expect("tempdir");
    let state = tempfile::tempdir().expect("tempdir");
    write_replay(replays.path(), "old.StormReplay", secs(900));
    write_replay(replays.path(), "good.StormReplay", secs(800));
    write_replay(replays.path(), "bad.StormReplay", secs(700));
    write_replay(replays.path(), "notes.txt", secs(700));

    let ledger_path = state.path().join("uploaded.json");
    let mut ledger = Ledger::open(&ledger_path).expect("ledger");
    ledger
        .record("old.StormReplay", Outcome::Uploaded)
        .expect("seed");

    let uploader = Uploader::new(server.base("mixed")).with_backoff_unit(FAST);
    let dirs = [replays.path().to_path_buf()];
    let progress = Progress::default();
    backfill::run(&dirs, &uploader, &mut ledger, &progress).await;

    let sent: BTreeSet<String> = server.filenames().into_iter().collect();
    assert_eq!(
        sent,
        BTreeSet::from([
            "bad.StormReplay".to_string(),
            "good.StormReplay".to_string()
        ])
    );
    assert_eq!(progress.total(), 2);
    assert_eq!(progress.done(), 2);
    assert_eq!(progress.uploaded(), 1);
    assert_eq!(progress.rejected(), 1);
    assert_eq!(progress.failed(), 0);
    let expected = vec![("bad.StormReplay".to_string(), REJECT_BODY.to_string())];
    assert_eq!(progress.rejected_files(), expected);
    assert_eq!(ledger.rejected(), expected);

    // Second start: everything is in the ledger (rejected included), nothing is re-sent.
    drop(ledger);
    let mut ledger = Ledger::open(&ledger_path).expect("reopen");
    let again = Progress::default();
    backfill::run(&dirs, &uploader, &mut ledger, &again).await;
    assert_eq!(server.hits().len(), 2);
    assert_eq!(again.total(), 0);
    assert_eq!(ledger.rejected(), expected);
}

#[tokio::test(flavor = "multi_thread")]
async fn backfill_continues_after_rejected_file() {
    let server = spawn_server().await;
    let a = tempfile::tempdir().expect("tempdir");
    let b = tempfile::tempdir().expect("tempdir");
    let state = tempfile::tempdir().expect("tempdir");
    // The corrupt replay is the oldest, so it is among the first sent.
    write_replay(a.path(), "bad-corrupt.StormReplay", secs(1000));
    for i in 0..6u64 {
        let dir = if i % 2 == 0 { a.path() } else { b.path() };
        write_replay(dir, &format!("good-{i}.StormReplay"), secs(900 - i * 10));
    }

    let mut ledger = Ledger::open(&state.path().join("uploaded.json")).expect("ledger");
    let uploader = Uploader::new(server.base("mixed")).with_backoff_unit(FAST);
    let progress = Progress::default();
    backfill::run(
        &[a.path().to_path_buf(), b.path().to_path_buf()],
        &uploader,
        &mut ledger,
        &progress,
    )
    .await;

    assert_eq!(progress.total(), 7);
    assert_eq!(progress.done(), 7);
    assert_eq!(progress.rejected(), 1);
    assert_eq!(progress.uploaded(), 6);
    let sent = server.filenames();
    assert_eq!(sent.len(), 7);
    for i in 0..6 {
        assert!(ledger.contains(&format!("good-{i}.StormReplay")));
    }
    // Oldest first: with 4 in flight, the oldest file is among the first 4 requests.
    let first_four = &sent[..4];
    assert!(
        first_four.iter().any(|n| n == "bad-corrupt.StormReplay"),
        "order: {sent:?}"
    );
    assert!(ledger.contains("bad-corrupt.StormReplay"));
}

#[tokio::test(flavor = "multi_thread")]
async fn backfill_skips_file_still_being_written() {
    let server = spawn_server().await;
    let replays = tempfile::tempdir().expect("tempdir");
    let state = tempfile::tempdir().expect("tempdir");
    // Touched just now: may still be written by the game, left to the watcher / next run.
    write_replay(replays.path(), "live.StormReplay", Duration::ZERO);
    write_replay(replays.path(), "dup.StormReplay", secs(600));

    let mut ledger = Ledger::open(&state.path().join("uploaded.json")).expect("ledger");
    let uploader = Uploader::new(server.base("409")).with_backoff_unit(FAST);
    let progress = Progress::default();
    backfill::run(
        &[replays.path().to_path_buf()],
        &uploader,
        &mut ledger,
        &progress,
    )
    .await;

    assert_eq!(server.filenames(), vec!["dup.StormReplay"]);
    assert_eq!(progress.total(), 2);
    assert_eq!(progress.done(), 2);
    assert_eq!(progress.skipped(), 1);
    assert_eq!(progress.duplicates(), 1);
    assert_eq!(progress.rejected(), 0);
    assert!(!ledger.contains("live.StormReplay"));
    assert!(ledger.contains("dup.StormReplay"));
    assert!(ledger.rejected().is_empty());
}
