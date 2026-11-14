//! Persisted record of the replay files already handled, keyed by file name.
//!
//! It only spares re-sending the whole archive at every start; the server does the real
//! dedup (content hash). Files that failed with a retryable error are never recorded, so
//! the next start retries them. Rejected files are kept with their reason so the UI can list
//! them once instead of re-sending them forever.
//!
//! Writes are atomic: compact JSON goes to a temp file unique to that write
//! (`<path>.<pid>.<seq>.tmp`), is synced to disk, then renamed over `<path>`. A crash leaves
//! either the old or the new file, plus at worst a stale temp file, removed by the next
//! [`Ledger::open`]. Two writes may overlap (an async flush still running on the blocking pool
//! when the backfill is cancelled and flushes again on drop): each has its own temp file, so
//! both are complete JSON, and every snapshot carries a sequence number taken when it is
//! serialised; the rename happens under a mutex and is skipped when a newer snapshot is already
//! published, so an older snapshot never replaces a newer one. [`Ledger::record`] writes through; bulk callers (the backfill)
//! use [`Ledger::record_deferred`] and [`Ledger::flush`] periodically, so a crash only loses
//! the last unflushed records, which is harmless: they are re-sent and the server answers 409.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

const FORMAT_VERSION: u32 = 1;

/// Final outcome of a replay, as persisted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", content = "reason", rename_all = "snake_case")]
pub enum Outcome {
    Uploaded,
    Duplicate,
    /// Refused by the server as bad content (corrupt replay, ...): never re-sent.
    Rejected(String),
}

/// On-disk shape (`version` is written for future migrations; unknown fields are ignored).
#[derive(Debug, Deserialize)]
struct LedgerFile {
    entries: BTreeMap<String, Outcome>,
}

#[derive(Serialize)]
struct LedgerFileRef<'a> {
    version: u32,
    entries: &'a BTreeMap<String, Outcome>,
}

#[derive(Debug)]
pub struct Ledger {
    path: PathBuf,
    entries: BTreeMap<String, Outcome>,
    /// In-memory entries not yet written to disk.
    dirty: bool,
    /// Sequence number of the next serialised snapshot.
    next_seq: u64,
    /// Sequence number of the snapshot currently on disk (shared with in-flight writes).
    published: Arc<Mutex<u64>>,
}

impl Ledger {
    /// Loads the ledger at `path`; a missing file is an empty ledger. Stale temp files left by
    /// a crash are removed. A corrupt ledger file is an `InvalidData` error.
    pub fn open(path: &Path) -> io::Result<Ledger> {
        remove_stale_temps(path);
        let entries = match fs::read(path) {
            Ok(bytes) => {
                let file: LedgerFile = serde_json::from_slice(&bytes)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                file.entries
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(e),
        };
        Ok(Ledger {
            path: path.to_path_buf(),
            entries,
            dirty: false,
            next_seq: 1,
            published: Arc::new(Mutex::new(0)),
        })
    }

    /// True when `name` already has a final outcome (uploaded, duplicate or rejected).
    pub fn contains(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    /// Records the outcome of `name` and persists the ledger atomically (write-through).
    /// On a write error the entry is rolled back in memory.
    pub fn record(&mut self, name: &str, outcome: Outcome) -> io::Result<()> {
        let previous = self.entries.insert(name.to_string(), outcome);
        self.dirty = true;
        let result = self.flush();
        if result.is_err() {
            match previous {
                Some(old) => self.entries.insert(name.to_string(), old),
                None => self.entries.remove(name),
            };
        }
        result
    }

    /// Records the outcome of `name` in memory only; persisted by the next [`Ledger::flush`]
    /// (or write-through [`Ledger::record`]).
    pub fn record_deferred(&mut self, name: &str, outcome: Outcome) {
        self.entries.insert(name.to_string(), outcome);
        self.dirty = true;
    }

    /// True when some records are not on disk yet.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Writes the ledger atomically if it has unflushed records (blocking file I/O).
    pub fn flush(&mut self) -> io::Result<()> {
        if !self.dirty {
            return Ok(());
        }
        let (json, seq) = self.serialize()?;
        write_atomic(&self.path, &json, seq, &self.published)?;
        self.dirty = false;
        Ok(())
    }

    /// [`Ledger::flush`] with the file I/O on tokio's blocking pool: serialization (cheap)
    /// stays on the caller, the write + fsync + rename do not block an async worker.
    pub async fn flush_async(&mut self) -> io::Result<()> {
        if !self.dirty {
            return Ok(());
        }
        let (json, seq) = self.serialize()?;
        let path = self.path.clone();
        let published = self.published.clone();
        tokio::task::spawn_blocking(move || write_atomic(&path, &json, seq, &published))
            .await
            .map_err(io::Error::other)??;
        self.dirty = false;
        Ok(())
    }

    /// `(file name, server reason)` of every rejected replay, sorted by name.
    pub fn rejected(&self) -> Vec<(String, String)> {
        self.entries
            .iter()
            .filter_map(|(name, outcome)| match outcome {
                Outcome::Rejected(reason) => Some((name.clone(), reason.clone())),
                _ => None,
            })
            .collect()
    }

    /// The current entries as JSON, with the snapshot's sequence number.
    fn serialize(&mut self) -> io::Result<(Vec<u8>, u64)> {
        let json = serde_json::to_vec(&LedgerFileRef {
            version: FORMAT_VERSION,
            entries: &self.entries,
        })
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let seq = self.next_seq;
        self.next_seq += 1;
        Ok((json, seq))
    }
}

/// Writes snapshot `seq` to its own temp file, then publishes it unless a newer snapshot is
/// already on disk. Sequential publication is enforced by `published` (held across the rename).
fn write_atomic(path: &Path, json: &[u8], seq: u64, published: &Mutex<u64>) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let tmp = tmp_path(path, seq);
    let written = (|| {
        let mut f = File::create(&tmp)?;
        f.write_all(json)?;
        f.sync_all()
    })();
    let result = written.and_then(|()| {
        // A panic while holding the lock cannot corrupt a plain sequence number.
        let mut last = published.lock().unwrap_or_else(|p| p.into_inner());
        if seq <= *last {
            return Ok(()); // a newer snapshot is already published: drop this one
        }
        rename_with_retry(&tmp, path)?;
        *last = seq;
        Ok(())
    });
    if tmp.exists() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// `<path>.<pid>.<seq>.tmp`: unique per write, even across two processes.
fn tmp_path(path: &Path, seq: u64) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{}.{seq}.tmp", std::process::id()));
    PathBuf::from(name)
}

/// Removes the temp files a crash may have left next to `path` (`<name>.tmp`, `<name>.*.tmp`).
fn remove_stale_temps(path: &Path) {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return;
    };
    let dir = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    let prefix = format!("{}.", name.to_string_lossy());
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let file = entry.file_name();
        let file = file.to_string_lossy();
        if file.starts_with(&prefix) && file.ends_with(".tmp") {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// `fs::rename` replaces the destination on Windows and Unix. On Windows it can fail
/// transiently while another process (antivirus, indexer) holds the destination open.
fn rename_with_retry(from: &Path, to: &Path) -> io::Result<()> {
    const ATTEMPTS: u32 = 5;
    let mut attempt = 1;
    loop {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) if attempt < ATTEMPTS && e.kind() == io::ErrorKind::PermissionDenied => {
                std::thread::sleep(Duration::from_millis(20 * u64::from(attempt)));
                attempt += 1;
            }
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn an_older_snapshot_never_replaces_a_newer_one() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("uploaded.json");
        let published = Mutex::new(0);
        write_atomic(&path, b"{\"seq\":2}", 2, &published).expect("write 2");
        // the older snapshot finishes last: it must be dropped, and its temp file removed
        write_atomic(&path, b"{\"seq\":1}", 1, &published).expect("write 1");
        assert_eq!(fs::read(&path).expect("read"), b"{\"seq\":2}");
        assert_eq!(*published.lock().unwrap(), 2);
        let leftovers = fs::read_dir(dir.path()).expect("list").count();
        assert_eq!(leftovers, 1, "no temp file left behind");
        write_atomic(&path, b"{\"seq\":3}", 3, &published).expect("write 3");
        assert_eq!(fs::read(&path).expect("read"), b"{\"seq\":3}");
    }
}
