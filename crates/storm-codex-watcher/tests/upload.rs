#![allow(clippy::expect_used)]

mod common;

use std::time::Duration;

use common::{spawn_server, write_replay, REJECT_BODY};
use storm_codex_watcher::backfill::{self, Progress};
use storm_codex_watcher::ledger::Ledger;
use storm_codex_watcher::upload::{encode_filename, UploadResult, Uploader, MAX_ATTEMPTS};

const OLD: Duration = Duration::from_secs(600);
/// Keeps the retry tests fast: backoff is `unit * 2^n` instead of `1 s * 2^n`.
const FAST: Duration = Duration::from_millis(1);

#[tokio::test(flavor = "multi_thread")]
async fn upload_maps_status_codes() {
    let server = spawn_server().await;
    let dir = tempfile::tempdir().expect("tempdir");
    let replay = write_replay(dir.path(), "game.StormReplay", OLD);

    let up = |mode: &str| Uploader::new(server.base(mode)).with_backoff_unit(FAST);

    assert_eq!(up("200").upload(&replay).await, UploadResult::Uploaded);
    assert_eq!(up("409").upload(&replay).await, UploadResult::Duplicate);
    match up("422").upload(&replay).await {
        UploadResult::Rejected(body) => assert_eq!(body, REJECT_BODY),
        other => panic!("expected Rejected, got {other:?}"),
    }
    assert!(matches!(
        up("500").upload(&replay).await,
        UploadResult::Retryable(_)
    ));

    // 4xx is never retried; the body really is the file bytes.
    let hits = server.hits();
    assert_eq!(hits.iter().filter(|h| h.mode == "422").count(), 1);
    assert!(hits.iter().all(|h| h.body_len > 0));
    assert!(hits.iter().all(|h| h.filename == "game.StormReplay"));
    // A trailing slash on the base URL does not produce `//api/upload`.
    let slash = Uploader::new(format!("{}/", server.base("200"))).with_backoff_unit(FAST);
    assert_eq!(slash.upload(&replay).await, UploadResult::Uploaded);
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_retries_then_reports_failure_without_recording() {
    let server = spawn_server().await;
    let dir = tempfile::tempdir().expect("tempdir");
    write_replay(dir.path(), "flaky.StormReplay", OLD);
    let ledger_path = dir.path().join("ledger").join("uploaded.json");

    let uploader = Uploader::new(server.base("500")).with_backoff_unit(FAST);
    let mut ledger = Ledger::open(&ledger_path).expect("ledger");
    let progress = Progress::default();
    backfill::run(
        &[dir.path().to_path_buf()],
        &uploader,
        &mut ledger,
        &progress,
    )
    .await;

    assert_eq!(server.hits().len(), MAX_ATTEMPTS as usize);
    assert_eq!(MAX_ATTEMPTS, 5);
    assert_eq!(progress.total(), 1);
    assert_eq!(progress.done(), 1);
    assert_eq!(progress.failed(), 1);
    assert_eq!(progress.uploaded(), 0);
    assert_eq!(progress.failed_files()[0].0, "flaky.StormReplay");
    // Not recorded: the next start retries it.
    assert!(!ledger.contains("flaky.StormReplay"));
    drop(ledger);
    assert!(!Ledger::open(&ledger_path)
        .expect("reopen")
        .contains("flaky.StormReplay"));
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_percent_encodes_non_ascii_filename() {
    assert_eq!(
        encode_filename("Tëst (1)+x_y-z.StormReplay"),
        "T%C3%ABst (1)%2Bx_y-z.StormReplay"
    );
    assert_eq!(
        encode_filename("Ваш.StormReplay"),
        "%D0%92%D0%B0%D1%88.StormReplay"
    );

    let server = spawn_server().await;
    let dir = tempfile::tempdir().expect("tempdir");
    let replay = write_replay(dir.path(), "Tëst (1)+x_y-z.StormReplay", OLD);
    let uploader = Uploader::new(server.base("200")).with_backoff_unit(FAST);
    assert_eq!(uploader.upload(&replay).await, UploadResult::Uploaded);
    assert_eq!(
        server.filenames(),
        vec!["T%C3%ABst (1)%2Bx_y-z.StormReplay"]
    );
}
