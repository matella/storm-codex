//! Persisted record of the replay files already handled, keyed by file name.
//!
//! It only spares re-sending the whole archive at every start; the server does the real
//! dedup (content hash). Files that failed with a retryable error are never recorded, so
//! the next start retries them. Rejected files are kept with their reason so the UI can list
//! them once instead of re-sending them forever.
//!
//! Writes are atomic: compact JSON goes to `<path>.tmp`, is synced to disk, then renamed over
//! `<path>`. A crash leaves either the old or the new file, plus at worst a stale `.tmp` that
//! the next write overwrites. [`Ledger::record`] writes through; bulk callers (the backfill)
//! use [`Ledger::record_deferred`] and [`Ledger::flush`] periodically, so a crash only loses
//! the last unflushed records, which is harmless: they are re-sent and the server answers 409.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
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
}

impl Ledger {
    /// Loads the ledger at `path`; a missing file is an empty ledger. A stale `<path>.tmp`
    /// left by a crash is ignored. A corrupt ledger file is an `InvalidData` error.
    pub fn open(path: &Path) -> io::Result<Ledger> {
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
        write_atomic(&self.path, &self.serialize()?)?;
        self.dirty = false;
        Ok(())
    }

    /// [`Ledger::flush`] with the file I/O on tokio's blocking pool: serialization (cheap)
    /// stays on the caller, the write + fsync + rename do not block an async worker.
    pub async fn flush_async(&mut self) -> io::Result<()> {
        if !self.dirty {
            return Ok(());
        }
        let json = self.serialize()?;
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || write_atomic(&path, &json))
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

    fn serialize(&self) -> io::Result<Vec<u8>> {
        serde_json::to_vec(&LedgerFileRef {
            version: FORMAT_VERSION,
            entries: &self.entries,
        })
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
}

fn write_atomic(path: &Path, json: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let tmp = tmp_path(path);
    {
        // `create` truncates a stale temp file left by a crash.
        let mut f = File::create(&tmp)?;
        f.write_all(json)?;
        f.sync_all()?;
    }
    rename_with_retry(&tmp, path)
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".tmp");
    PathBuf::from(name)
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
