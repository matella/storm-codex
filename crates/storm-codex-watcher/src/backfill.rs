//! Archive backfill: uploads every replay of the given folders not yet in the ledger.
//!
//! Files are sent oldest first (by mtime) so the history fills chronologically, 4 at a time.
//! A file that may still be written (modified within the last 2 s, or whose size/mtime
//! changes while it is read) is skipped for this run, not recorded: the live watcher or the
//! next run picks it up. Rejected files are recorded with their reason and never stop the
//! run; retryable failures are counted but not recorded, so the next start retries them.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use futures_util::stream::{self, StreamExt};

use crate::detector::is_storm_replay;
use crate::ledger::{Ledger, Outcome};
use crate::upload::{file_name, UploadResult, Uploader};

/// Uploads in flight at once.
pub const CONCURRENCY: usize = 4;
/// A file modified more recently than this may still be written by the game.
const RECENT_WRITE_GRACE: Duration = Duration::from_secs(2);

/// Live counters of a backfill run, readable from another task while it runs.
#[derive(Debug, Default)]
pub struct Progress {
    total: AtomicUsize,
    done: AtomicUsize,
    uploaded: AtomicUsize,
    duplicates: AtomicUsize,
    rejected: AtomicUsize,
    failed: AtomicUsize,
    skipped: AtomicUsize,
    rejected_files: Mutex<Vec<(String, String)>>,
    failed_files: Mutex<Vec<(String, String)>>,
}

impl Progress {
    /// Files to process in this run (replays not yet in the ledger).
    pub fn total(&self) -> usize {
        self.total.load(Ordering::Relaxed)
    }
    /// Files processed so far, whatever their outcome.
    pub fn done(&self) -> usize {
        self.done.load(Ordering::Relaxed)
    }
    pub fn uploaded(&self) -> usize {
        self.uploaded.load(Ordering::Relaxed)
    }
    pub fn duplicates(&self) -> usize {
        self.duplicates.load(Ordering::Relaxed)
    }
    pub fn rejected(&self) -> usize {
        self.rejected.load(Ordering::Relaxed)
    }
    /// Retryable failures (network / 5xx after every attempt): retried at the next start.
    pub fn failed(&self) -> usize {
        self.failed.load(Ordering::Relaxed)
    }
    /// Files left for later because they may still be written.
    pub fn skipped(&self) -> usize {
        self.skipped.load(Ordering::Relaxed)
    }
    /// `(file name, server reason)` of the files rejected during this run.
    pub fn rejected_files(&self) -> Vec<(String, String)> {
        lock(&self.rejected_files).clone()
    }
    /// `(file name, last error)` of the files that failed during this run.
    pub fn failed_files(&self) -> Vec<(String, String)> {
        lock(&self.failed_files).clone()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // A panic while holding the lock cannot leave these plain lists inconsistent.
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// What happened to one candidate file.
enum FileOutcome {
    Sent(UploadResult),
    /// Possibly still being written: left for the watcher / next run.
    Unstable,
}

/// Uploads every replay of `dirs` (non-recursive) missing from `ledger`, updating `progress`.
pub async fn run(dirs: &[PathBuf], uploader: &Uploader, ledger: &mut Ledger, progress: &Progress) {
    let candidates = collect_candidates(dirs, ledger);
    progress.total.store(candidates.len(), Ordering::Relaxed);
    tracing::info!(files = candidates.len(), "backfill started");

    let mut results = stream::iter(candidates)
        .map(|path| async move {
            let outcome = process(uploader, &path).await;
            (file_name(&path), outcome)
        })
        .buffer_unordered(CONCURRENCY);

    while let Some((name, outcome)) = results.next().await {
        let recorded = match outcome {
            FileOutcome::Unstable => {
                progress.skipped.fetch_add(1, Ordering::Relaxed);
                None
            }
            FileOutcome::Sent(UploadResult::Uploaded) => {
                progress.uploaded.fetch_add(1, Ordering::Relaxed);
                Some(Outcome::Uploaded)
            }
            FileOutcome::Sent(UploadResult::Duplicate) => {
                progress.duplicates.fetch_add(1, Ordering::Relaxed);
                Some(Outcome::Duplicate)
            }
            FileOutcome::Sent(UploadResult::Rejected(reason)) => {
                tracing::warn!(file = %name, %reason, "replay rejected by the server");
                progress.rejected.fetch_add(1, Ordering::Relaxed);
                lock(&progress.rejected_files).push((name.clone(), reason.clone()));
                Some(Outcome::Rejected(reason))
            }
            FileOutcome::Sent(UploadResult::Retryable(error)) => {
                tracing::warn!(file = %name, %error, "upload failed, retried at next start");
                progress.failed.fetch_add(1, Ordering::Relaxed);
                lock(&progress.failed_files).push((name.clone(), error));
                None
            }
        };
        if let Some(outcome) = recorded {
            if let Err(e) = ledger.record(&name, outcome) {
                // Not fatal: the server dedups, the file is only re-sent at the next start.
                tracing::warn!(file = %name, error = %e, "could not persist the upload ledger");
            }
        }
        progress.done.fetch_add(1, Ordering::Relaxed);
    }

    tracing::info!(
        uploaded = progress.uploaded(),
        duplicates = progress.duplicates(),
        rejected = progress.rejected(),
        failed = progress.failed(),
        skipped = progress.skipped(),
        "backfill finished"
    );
}

/// Replays of `dirs` not in the ledger, oldest first, one per file name.
fn collect_candidates(dirs: &[PathBuf], ledger: &Ledger) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut candidates: Vec<(SystemTime, PathBuf)> = Vec::new();
    for dir in dirs {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) => {
                tracing::warn!(dir = %dir.display(), error = %e, "cannot read replay folder");
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !is_storm_replay(&path) {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let name = file_name(&path);
            if ledger.contains(&name) || !seen.insert(name) {
                continue;
            }
            let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            candidates.push((mtime, path));
        }
    }
    candidates.sort();
    candidates.into_iter().map(|(_, path)| path).collect()
}

/// Reads the file only if it looks complete, then uploads it.
async fn process(uploader: &Uploader, path: &Path) -> FileOutcome {
    let Some(before) = stamp(path).await else {
        return FileOutcome::Unstable;
    };
    // An mtime in the future (clock skew) is not "recent": it would otherwise never be sent;
    // the size/mtime comparison around the read still guards it.
    let recent = SystemTime::now()
        .duration_since(before.1)
        .is_ok_and(|age| age < RECENT_WRITE_GRACE);
    if recent {
        return FileOutcome::Unstable;
    }
    let Ok(bytes) = tokio::fs::read(path).await else {
        return FileOutcome::Unstable;
    };
    let after = stamp(path).await;
    if after != Some(before) || bytes.len() as u64 != before.0 {
        return FileOutcome::Unstable;
    }
    FileOutcome::Sent(uploader.upload_bytes(&file_name(path), bytes).await)
}

/// `(size, mtime)` of a file, `None` if it vanished or cannot be read.
async fn stamp(path: &Path) -> Option<(u64, SystemTime)> {
    let meta = tokio::fs::metadata(path).await.ok()?;
    Some((meta.len(), meta.modified().ok()?))
}
