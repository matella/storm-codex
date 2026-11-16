#![allow(clippy::expect_used)]

use std::fs::OpenOptions;
use std::io::Write;
use std::time::Duration;

use storm_codex_watcher::watch;
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout};

fn append(path: &std::path::Path, bytes: &[u8]) {
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("open replay");
    f.write_all(bytes).expect("append");
    f.sync_all().expect("sync");
}

#[tokio::test(flavor = "multi_thread")]
async fn watch_waits_until_file_is_stable() {
    let root = tempfile::tempdir().expect("tempdir");
    // Recursive: the replay lands in a sub-folder of the watched one.
    let nested = root.path().join("1").join("2-Hero-1-3");
    std::fs::create_dir_all(&nested).expect("mkdir");

    let (tx, mut rx) = mpsc::unbounded_channel();
    let handle = watch::start(&[root.path().to_path_buf()], tx).expect("start watch");
    sleep(Duration::from_millis(300)).await; // let the OS watch arm

    let replay = nested.join("2026-11-12 Cursed Hollow.StormReplay");
    append(&nested.join("ignored.txt"), b"not a replay");
    append(&replay, &[1u8; 4096]);
    sleep(Duration::from_millis(1500)).await;
    assert!(
        rx.try_recv().is_err(),
        "a half-written replay must not be emitted"
    );

    append(&replay, &[2u8; 4096]);
    let got = timeout(Duration::from_secs(10), rx.recv())
        .await
        .expect("replay emitted once stable")
        .expect("channel open");
    assert_eq!(got.file_name(), replay.file_name());
    assert_eq!(std::fs::metadata(&got).expect("stat").len(), 8192);

    // Exactly one event for the two chunks.
    assert!(
        timeout(Duration::from_secs(3), rx.recv()).await.is_err(),
        "no second event"
    );
    handle.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn watch_rejects_missing_dir() {
    let root = tempfile::tempdir().expect("tempdir");
    let (tx, _rx) = mpsc::unbounded_channel();
    assert!(watch::start(&[root.path().join("missing")], tx).is_err());
}
