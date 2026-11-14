//! Backup archives (`.stormcodex`): a zip holding `db.dump` (`pg_dump -Fc --data-only` of the
//! app database: rows only, the migration ledger included) and the whole `archive/` folder (the
//! raw replays, the source of truth).
//!
//! Export streams every file into the zip (`Stored`: replays and the custom-format dump are
//! already compressed), never loading a file in memory. Import copies the chosen file to a
//! private one, validates that copy (entry names, kinds, sizes) and unpacks it within the
//! validated budget, restores the dump's rows into a staging database whose schema the server's
//! own migrations created (under an unprivileged role), then stops the server, swaps the archive
//! folder and promotes the database, with a journal for crash recovery ([`recover_import`]).

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use storm_codex_supervisor::DataDir;

use crate::supervise::{Stack, StopBudget};

/// Dump entry name.
pub const DUMP_ENTRY: &str = "db.dump";
/// Folder entry holding the replays.
pub const ARCHIVE_ENTRY: &str = "archive";
pub const BACKUP_EXTENSION: &str = "stormcodex";
/// Sanity limits on an imported zip.
pub const MAX_ENTRIES: usize = 2_000_000;
pub const MAX_ENTRY_BYTES: u64 = 256 << 30;
/// Deflated entries may not expand more than this (zip bomb).
pub const MAX_RATIO: u64 = 200;
/// Free space kept on the data drive besides the unpacked size.
pub const IMPORT_FREE_MARGIN: u64 = 1 << 30;

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("cannot read {path} as a zip file: {source}")]
    Zip {
        path: PathBuf,
        #[source]
        source: zip::result::ZipError,
    },
    #[error("not a valid Storm Codex backup: {0}")]
    Invalid(String),
    #[error("database dump failed: {0}")]
    Dump(String),
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> BackupError + '_ {
    move |source| BackupError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// What a validated backup holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupContents {
    pub dump_bytes: u64,
    pub archive_files: u64,
    /// Uncompressed size of every entry.
    pub total_bytes: u64,
}

/// `storm-codex-backup-YYYY-MM-DD.stormcodex` for the UTC day of `unix_secs`.
pub fn backup_file_name(unix_secs: u64) -> String {
    let (y, m, d) = civil_from_days((unix_secs / 86_400) as i64);
    format!("storm-codex-backup-{y:04}-{m:02}-{d:02}.{BACKUP_EXTENSION}")
}

/// Days since 1970-01-01 -> (year, month, day), proleptic Gregorian (H. Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// Writes the backup to `out`: `dump` writes the database dump to a file in `work_dir`, then
/// the zip is built in `<out>.partial` (created new: an existing file there is never
/// overwritten) and renamed to `out` once complete; `out` itself is the path the user chose in
/// the save dialog, which asked before replacing a file. The dump is taken
/// first, the archive copied after: every replay the dump references is in the zip (a replay
/// imported meanwhile is an orphan file, harmless).
pub fn export(
    dump: impl FnOnce(&Path) -> Result<(), String>,
    archive_dir: &Path,
    work_dir: &Path,
    out: &Path,
) -> Result<BackupContents, BackupError> {
    std::fs::create_dir_all(work_dir).map_err(io_err(work_dir))?;
    let dump_file = work_dir.join(DUMP_ENTRY);
    dump(&dump_file).map_err(BackupError::Dump)?;
    let files = list_files(archive_dir)?;
    let partial = partial_path(out);
    let file = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)
    {
        Ok(f) => f,
        Err(e) => {
            let _ = std::fs::remove_file(&dump_file);
            return Err(io_err(&partial)(e));
        }
    };
    let result = write_zip(file, &partial, &dump_file, archive_dir, &files);
    let _ = std::fs::remove_file(&dump_file);
    match result {
        Ok(contents) => {
            std::fs::rename(&partial, out).map_err(io_err(out))?;
            Ok(contents)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&partial);
            Err(e)
        }
    }
}

fn partial_path(out: &Path) -> PathBuf {
    let mut name = out.file_name().unwrap_or_default().to_os_string();
    name.push(".partial");
    out.with_file_name(name)
}

/// Every file under `dir` (sorted, relative, `/`-separated). A missing folder is empty.
fn list_files(dir: &Path) -> Result<Vec<String>, BackupError> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), BackupError> {
        let entries = std::fs::read_dir(dir).map_err(io_err(dir))?;
        for entry in entries {
            let entry = entry.map_err(io_err(dir))?;
            let path = entry.path();
            let kind = entry.file_type().map_err(io_err(&path))?;
            if kind.is_dir() {
                walk(root, &path, out)?;
            } else if kind.is_file() {
                let rel = path.strip_prefix(root).unwrap_or(&path);
                let parts: Option<Vec<&str>> =
                    rel.components().map(|c| c.as_os_str().to_str()).collect();
                let Some(parts) = parts else {
                    return Err(BackupError::Invalid(format!(
                        "file name is not valid Unicode: {}",
                        path.display()
                    )));
                };
                out.push(parts.join("/"));
            }
            // Symbolic links are skipped: the archive only holds regular files.
        }
        Ok(())
    }
    let mut out = Vec::new();
    if dir.is_dir() {
        walk(dir, dir, &mut out)?;
    }
    out.sort();
    Ok(out)
}

fn write_zip(
    file: File,
    partial: &Path,
    dump_file: &Path,
    archive_dir: &Path,
    files: &[String],
) -> Result<BackupContents, BackupError> {
    let zip_err = |source| BackupError::Zip {
        path: partial.to_path_buf(),
        source,
    };
    let mut zip = zip::ZipWriter::new(io::BufWriter::new(file));
    let add = |zip: &mut zip::ZipWriter<io::BufWriter<File>>,
               name: &str,
               path: &Path|
     -> Result<u64, BackupError> {
        let mut src = File::open(path).map_err(io_err(path))?;
        let size = src.metadata().map_err(io_err(path))?.len();
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .large_file(size >= u64::from(u32::MAX));
        zip.start_file(name, opts).map_err(zip_err)?;
        io::copy(&mut src, zip).map_err(io_err(path))
    };
    let dump_bytes = add(&mut zip, DUMP_ENTRY, dump_file)?;
    let mut total_bytes = dump_bytes;
    for rel in files {
        let path = rel
            .split('/')
            .fold(archive_dir.to_path_buf(), |p, c| p.join(c));
        total_bytes += add(&mut zip, &format!("{ARCHIVE_ENTRY}/{rel}"), &path)?;
    }
    let writer = zip.finish().map_err(zip_err)?;
    let file = writer
        .into_inner()
        .map_err(|e| io_err(partial)(e.into_error()))?;
    file.sync_all().map_err(io_err(partial))?;
    Ok(BackupContents {
        dump_bytes,
        archive_files: files.len() as u64,
        total_bytes,
    })
}

/// Where an entry goes.
#[derive(Debug, PartialEq, Eq)]
enum Entry {
    Dump,
    /// A folder under `archive/` (or `archive/` itself), relative parts.
    ArchiveDir(Vec<String>),
    ArchiveFile(Vec<String>),
}

const RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9", "conin$",
    "conout$", "clock$", "com¹", "com²", "com³", "lpt¹", "lpt²", "lpt³",
];

/// Classifies an entry name, refusing anything that could land outside the target folder or
/// that Windows would reinterpret: `\`, `:`, NUL, absolute names, empty / `.` / `..`
/// components, device names (`CON`, `nul.txt`), trailing dots or spaces.
fn classify(name: &str) -> Result<Entry, String> {
    let bad = |why: &str| Err(format!("unsafe entry {name:?} ({why})"));
    if name == DUMP_ENTRY {
        return Ok(Entry::Dump);
    }
    if name.contains(['\\', ':', '\0']) {
        return bad("forbidden character");
    }
    if name.starts_with('/') {
        return bad("absolute path");
    }
    let is_dir = name.ends_with('/');
    let trimmed = name.strip_suffix('/').unwrap_or(name);
    let mut parts = trimmed.split('/');
    if parts.next() != Some(ARCHIVE_ENTRY) {
        return Err(format!(
            "unexpected entry {name:?} (only db.dump and archive/ are allowed)"
        ));
    }
    let mut rel = Vec::new();
    for part in parts {
        if part.is_empty() || part == "." || part == ".." {
            return bad("empty, . or .. path component");
        }
        if part.ends_with(['.', ' ']) {
            return bad("trailing dot or space");
        }
        // Windows ignores spaces before the extension: `con .txt` is `CON`.
        let stem = part
            .split('.')
            .next()
            .unwrap_or(part)
            .trim_end()
            .to_ascii_lowercase();
        if RESERVED.contains(&stem.as_str()) {
            return bad("reserved Windows device name");
        }
        rel.push(part.to_string());
    }
    if is_dir {
        Ok(Entry::ArchiveDir(rel))
    } else if rel.is_empty() {
        bad("archive must be a folder")
    } else {
        Ok(Entry::ArchiveFile(rel))
    }
}

fn open_zip(file: &Path) -> Result<zip::ZipArchive<io::BufReader<File>>, BackupError> {
    let f = File::open(file).map_err(io_err(file))?;
    zip::ZipArchive::new(io::BufReader::new(f)).map_err(|source| BackupError::Zip {
        path: file.to_path_buf(),
        source,
    })
}

/// Magic bytes of a `pg_dump -Fc` file.
const PGDMP_MAGIC: &[u8] = b"PGDMP";

/// Checks every entry of `file` without extracting anything: names (see `classify`), regular
/// files only (no symlink, no encryption), entry count, sizes and compression ratio, exactly
/// one `db.dump` starting with the pg_dump magic.
pub fn validate(file: &Path) -> Result<BackupContents, BackupError> {
    let invalid = |m: String| Err(BackupError::Invalid(m));
    let mut zip = open_zip(file)?;
    if zip.len() > MAX_ENTRIES {
        return invalid(format!("{} entries (limit {MAX_ENTRIES})", zip.len()));
    }
    let zip_err = |source| BackupError::Zip {
        path: file.to_path_buf(),
        source,
    };
    let mut dumps = 0;
    let mut contents = BackupContents {
        dump_bytes: 0,
        archive_files: 0,
        total_bytes: 0,
    };
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(zip_err)?;
        let name = entry.name().map_err(zip_err)?.into_owned();
        let kind = classify(&name).map_err(BackupError::Invalid)?;
        if entry.enclosed_name().is_none() {
            return invalid(format!("unsafe entry {name:?}"));
        }
        if entry.is_symlink() {
            return invalid(format!("entry {name:?} is a symbolic link"));
        }
        if entry.encrypted() {
            return invalid(format!("entry {name:?} is encrypted"));
        }
        let size = entry.size();
        if size > MAX_ENTRY_BYTES {
            return invalid(format!("entry {name:?} is too large ({size} bytes)"));
        }
        if entry.compression() != zip::CompressionMethod::Stored
            && size
                > entry
                    .compressed_size()
                    .saturating_mul(MAX_RATIO)
                    .max(1 << 20)
        {
            return invalid(format!("entry {name:?} expands too much"));
        }
        match kind {
            Entry::Dump => {
                dumps += 1;
                let mut head = [0u8; 5];
                let ok = size >= head.len() as u64
                    && entry.read_exact(&mut head).is_ok()
                    && head == PGDMP_MAGIC;
                if !ok {
                    return invalid("db.dump is not a pg_dump custom-format file".into());
                }
                contents.dump_bytes = size;
            }
            Entry::ArchiveFile(_) => {
                if entry.is_dir() {
                    return invalid(format!("entry {name:?} is not a file"));
                }
                contents.archive_files += 1;
            }
            Entry::ArchiveDir(_) => {}
        }
        contents.total_bytes = contents.total_bytes.saturating_add(size);
    }
    if dumps != 1 {
        return invalid(format!("expected exactly one db.dump, found {dumps}"));
    }
    Ok(contents)
}

/// Copies exactly `size` bytes (the entry's declared size), refusing more.
fn copy_exact(from: impl Read, to: &mut impl Write, size: u64) -> io::Result<u64> {
    let mut limited = from.take(size.saturating_add(1));
    let n = io::copy(&mut limited, to)?;
    if n != size {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("entry holds {n} bytes, {size} declared"),
        ));
    }
    Ok(n)
}

/// Extracts a [`validate`]d backup: the dump to `dump_out`, the replays under `archive_out`
/// (which must not exist yet). Names are re-checked and the zip must stay within `budget` (what
/// [`validate`] counted: entries, bytes, one dump of the validated size); symlinks and
/// encrypted entries are refused again. Files are created new (never overwritten).
pub fn unpack(
    file: &Path,
    dump_out: &Path,
    archive_out: &Path,
    budget: &BackupContents,
) -> Result<(), BackupError> {
    if archive_out.exists() {
        return Err(io_err(archive_out)(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "target folder already exists",
        )));
    }
    let mut zip = open_zip(file)?;
    if zip.len() > MAX_ENTRIES {
        return Err(BackupError::Invalid(format!(
            "{} entries (limit {MAX_ENTRIES})",
            zip.len()
        )));
    }
    std::fs::create_dir_all(archive_out).map_err(io_err(archive_out))?;
    let zip_err = |source| BackupError::Zip {
        path: file.to_path_buf(),
        source,
    };
    let over = |what: &str| {
        Err(BackupError::Invalid(format!(
            "the file holds more {what} than was validated"
        )))
    };
    let mut seen = BackupContents {
        dump_bytes: 0,
        archive_files: 0,
        total_bytes: 0,
    };
    let mut dumps = 0;
    for i in 0..zip.len() {
        let entry = zip.by_index(i).map_err(zip_err)?;
        let name = entry.name().map_err(zip_err)?.into_owned();
        if entry.is_symlink() || entry.encrypted() {
            return Err(BackupError::Invalid(format!(
                "entry {name:?} is a symbolic link or encrypted"
            )));
        }
        let size = entry.size();
        seen.total_bytes = seen.total_bytes.saturating_add(size);
        if seen.total_bytes > budget.total_bytes {
            return over("bytes");
        }
        let target = match classify(&name).map_err(BackupError::Invalid)? {
            Entry::Dump => {
                dumps += 1;
                if dumps > 1 || size != budget.dump_bytes {
                    return over("dump data");
                }
                dump_out.to_path_buf()
            }
            Entry::ArchiveDir(parts) => {
                let dir = parts
                    .iter()
                    .fold(archive_out.to_path_buf(), |p, c| p.join(c));
                std::fs::create_dir_all(&dir).map_err(io_err(&dir))?;
                continue;
            }
            Entry::ArchiveFile(parts) => {
                seen.archive_files += 1;
                if seen.archive_files > budget.archive_files {
                    return over("files");
                }
                parts
                    .iter()
                    .fold(archive_out.to_path_buf(), |p, c| p.join(c))
            }
        };
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(io_err(parent))?;
        }
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(io_err(&target))?;
        copy_exact(entry, &mut out, size).map_err(io_err(&target))?;
        out.sync_all().map_err(io_err(&target))?;
    }
    if dumps != 1 {
        return Err(BackupError::Invalid("no db.dump".into()));
    }
    Ok(())
}

/// True when `dir` holds at least one file (any depth).
pub fn has_files(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| match e.file_type() {
        Ok(t) if t.is_dir() => has_files(&e.path()),
        Ok(_) => true,
        Err(_) => false,
    })
}

/// Refuses what is not a plain local file path: UNC shares (`\\server\share`), device and
/// verbatim paths (`\\?\`, `\\.\`), relative paths and, on Windows, mapped network drives.
pub fn check_local_path(path: &Path) -> Result<(), String> {
    let text = path.as_os_str().to_string_lossy();
    if text.starts_with(r"\\") || text.starts_with("//") || text.starts_with(r"\/") {
        return Err(format!(
            "{}: network and device paths are not supported; pick a file on a local drive",
            path.display()
        ));
    }
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let drive = match path.components().next() {
            Some(Component::Prefix(p)) => match p.kind() {
                Prefix::Disk(letter) => Some(letter),
                _ => None,
            },
            _ => None,
        };
        let Some(letter) = drive.filter(|_| path.is_absolute()) else {
            return Err(format!(
                "{}: not an absolute path on a local drive",
                path.display()
            ));
        };
        if is_network_drive(letter) {
            return Err(format!(
                "{}: network drives are not supported; pick a file on a local drive",
                path.display()
            ));
        }
    }
    #[cfg(not(windows))]
    if !path.is_absolute() {
        return Err(format!("{}: not an absolute path", path.display()));
    }
    Ok(())
}

/// `GetDriveTypeW` says `DRIVE_REMOTE` (a mapped network share).
#[cfg(windows)]
fn is_network_drive(letter: u8) -> bool {
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
    const DRIVE_REMOTE: u32 = 4;
    let root: Vec<u16> = format!("{}:\\", char::from(letter))
        .encode_utf16()
        .chain(Some(0))
        .collect();
    // SAFETY: `root` is a NUL-terminated UTF-16 string that outlives the call.
    unsafe { GetDriveTypeW(root.as_ptr()) == DRIVE_REMOTE }
}

/// Folder (data root) where an import is prepared; removed afterwards and at every start.
pub const IMPORT_WORK: &str = "import-work";
/// Present (data root) from just before the server is stopped for an import until the import
/// is committed or rolled back: [`recover_import`] reads it at the next start.
pub const IMPORT_JOURNAL: &str = "import-journal";
/// Private copy of the chosen backup, inside [`IMPORT_WORK`].
const PRIVATE_COPY: &str = "backup.zip";

/// Free space an import needs once its private copy is on disk: the unpacked size, about three
/// times the dump for the staging database (tables, indexes, WAL), and the margin.
pub fn import_space_needed(contents: &BackupContents) -> u64 {
    contents
        .total_bytes
        .saturating_add(contents.dump_bytes.saturating_mul(3))
        .saturating_add(IMPORT_FREE_MARGIN)
}

/// Copies `source` (`size` bytes, read before) to the new file `dest`; refused when the source
/// holds another number of bytes (it changed meanwhile).
pub fn copy_private(source: &Path, dest: &Path, size: u64) -> Result<(), BackupError> {
    let from = File::open(source).map_err(io_err(source))?;
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)
        .map_err(io_err(dest))?;
    copy_exact(io::BufReader::new(from), &mut out, size).map_err(|e| {
        BackupError::Invalid(format!(
            "{} changed while it was copied: {e}",
            source.display()
        ))
    })?;
    out.sync_all().map_err(io_err(dest))
}

/// `<target>.old`.
fn old_dir(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    target.with_file_name(name)
}

/// `std::fs::rename`, retried a few times: an antivirus or indexer may hold a file briefly.
pub fn rename_retrying(from: &Path, to: &Path) -> io::Result<()> {
    let mut attempt = 0;
    loop {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) if attempt >= 4 => return Err(e),
            Err(_) => {
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        }
    }
}

/// Swaps folder `new` in as `target`, keeping the replaced folder as `<target>.old` (removed
/// once the import is committed, put back by [`unswap_dir`] otherwise). Refused when
/// `<target>.old` already exists (never overwritten). On failure `target` is as before, unless
/// putting it back failed too: then `target` is missing and only `<target>.old` holds it.
pub fn swap_dir(
    new: &Path,
    target: &Path,
    rename: impl Fn(&Path, &Path) -> io::Result<()>,
) -> Result<(), BackupError> {
    let old = old_dir(target);
    if old.exists() {
        return Err(io_err(&old)(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "a folder from an earlier import is in the way; move it elsewhere first",
        )));
    }
    std::fs::create_dir_all(target).map_err(io_err(target))?;
    rename(target, &old).map_err(io_err(target))?;
    if let Err(e) = rename(new, target) {
        return match rename(&old, target) {
            Ok(()) => Err(io_err(target)(e)),
            Err(back) => Err(io_err(target)(io::Error::new(
                e.kind(),
                format!(
                    "{e}; putting the previous folder back from {} also failed: {back}",
                    old.display()
                ),
            ))),
        };
    }
    Ok(())
}

/// Undoes [`swap_dir`]: `<target>.old` goes back to `target` (the folder there, the imported
/// one, is removed). Nothing to do without `<target>.old`.
pub fn unswap_dir(target: &Path) -> Result<(), BackupError> {
    let old = old_dir(target);
    if !old.exists() {
        return Ok(());
    }
    if target.exists() {
        std::fs::remove_dir_all(target).map_err(io_err(target))?;
    }
    rename_retrying(&old, target).map_err(io_err(&old))
}

fn remove_file_if_exists(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(format!("{}: {e}", path.display())),
        _ => Ok(()),
    }
}

/// After the promote: the upload ledger is reset (it lists what the replaced database had
/// imported), the replaced archive removed, then the journal.
fn complete_import(data: &DataDir) -> Result<(), String> {
    remove_file_if_exists(&data.ledger())?;
    let old = old_dir(&data.archive());
    if old.exists() {
        std::fs::remove_dir_all(&old).map_err(|e| format!("{}: {e}", old.display()))?;
    }
    remove_file_if_exists(&data_root(data).join(IMPORT_JOURNAL))
}

/// Startup step (Postgres up, server not started): finishes or rolls back an import a crash
/// cut short, then removes the work folder.
/// - journal and staging database: not promoted, so roll back (archive put back from
///   `archive.old`, journal removed, staging dropped);
/// - journal, no staging database: promoted, so complete it (finish, ledger reset,
///   `archive.old` removed, journal removed);
/// - no journal: a staging database is an orphan (dropped); an `archive.old` next to an empty
///   `archive` is put back; an `archive.old` next to a non-empty `archive` is left alone.
pub fn recover_import(data: &DataDir, db: &dyn crate::supervise::Db) -> Result<(), String> {
    let root = data_root(data);
    let journal = root.join(IMPORT_JOURNAL);
    let staged = db.staging_exists().map_err(|e| e.to_string())?;
    let archive = data.archive();
    if journal.exists() {
        if staged {
            unswap_dir(&archive).map_err(|e| e.to_string())?;
            remove_file_if_exists(&journal)?;
            db.discard_import().map_err(|e| e.to_string())?;
        } else {
            db.finish_import().map_err(|e| e.to_string())?;
            complete_import(data)?;
        }
    } else {
        if staged {
            db.discard_import().map_err(|e| e.to_string())?;
        }
        if old_dir(&archive).exists() && !has_files(&archive) {
            unswap_dir(&archive).map_err(|e| e.to_string())?;
        }
    }
    let work = root.join(IMPORT_WORK);
    if work.exists() {
        std::fs::remove_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    }
    Ok(())
}

/// The data root (parent of `archive/`, `logs/`, ...).
fn data_root(data: &DataDir) -> PathBuf {
    data.archive()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

/// Exports the running stack's data to `out` (Postgres up; the server keeps running: the
/// dump is a consistent snapshot, taken before the archive is copied).
pub fn export_stack(stack: &Stack, out: &Path) -> Result<BackupContents, BackupError> {
    let work = data_root(stack.data()).join("export-work");
    let result = export(
        |p| stack.dump(p).map_err(|e| e.to_string()),
        &stack.data().archive(),
        &work,
        out,
    );
    let _ = std::fs::remove_dir_all(&work);
    result
}

/// Imports the backup `source` into the running stack (the caller asked the user first).
/// 1. `source` must be a local path; it is copied to a private file in `import-work` (free
///    space checked), and only that copy is validated and unpacked (within the validated
///    budget; free space for the unpacked data and the staging database checked).
/// 2. The dump's rows are restored into the staging database ([`Stack::stage_import`]: schema
///    from the server's `migrate`, TOC allow-list, unprivileged role) while the server keeps
///    running; a backup from a newer app is refused there.
/// 3. Journal written, server stopped, archive swapped, staging database promoted; the ledger
///    is reset, `archive.old` and the journal removed, the server restarted.
///
/// A failure before the database rename rolls everything back (archive, staging database) and
/// restarts the server. Once the rename has committed nothing is rolled back: the import is
/// completed (or left for the next start to complete). When the state cannot be read or the
/// replay folder cannot be put back, the server stays stopped and [`recover_import`] finishes
/// at the next start; a crash is handled the same way.
pub fn import_into(
    stack: &mut Stack,
    source: &Path,
    free_space: impl Fn(&Path) -> io::Result<u64>,
) -> Result<BackupContents, String> {
    import_into_with(stack, source, free_space, &rename_retrying)
}

/// How a folder is renamed (injected by the tests).
type Rename<'a> = &'a dyn Fn(&Path, &Path) -> io::Result<()>;

fn import_into_with(
    stack: &mut Stack,
    source: &Path,
    free_space: impl Fn(&Path) -> io::Result<u64>,
    rename: Rename<'_>,
) -> Result<BackupContents, String> {
    check_local_path(source)?;
    let data = stack.data().clone();
    let root = data_root(&data);
    let work = root.join(IMPORT_WORK);
    let clean = |work: &Path| -> Result<(), String> {
        if work.exists() {
            std::fs::remove_dir_all(work).map_err(|e| format!("{}: {e}", work.display()))?;
        }
        Ok(())
    };
    clean(&work)?;
    std::fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    let result = prepare(source, &work, &root, &free_space)
        .and_then(|contents| commit(stack, &work, &data, rename).map(|()| contents));
    let _ = clean(&work);
    result
}

fn check_space(
    free_space: &dyn Fn(&Path) -> io::Result<u64>,
    root: &Path,
    needed: u64,
) -> Result<(), String> {
    let free = free_space(root).map_err(|e| format!("cannot read the free space: {e}"))?;
    if free < needed {
        return Err(format!(
            "not enough free disk space: {needed} bytes needed, {free} bytes free"
        ));
    }
    Ok(())
}

/// Private copy, validation and unpacking into `work` (nothing else is touched).
fn prepare(
    source: &Path,
    work: &Path,
    root: &Path,
    free_space: &dyn Fn(&Path) -> io::Result<u64>,
) -> Result<BackupContents, String> {
    let size = std::fs::metadata(source)
        .map_err(|e| format!("{}: {e}", source.display()))?
        .len();
    check_space(free_space, root, size.saturating_add(IMPORT_FREE_MARGIN))?;
    let copy = work.join(PRIVATE_COPY);
    copy_private(source, &copy, size).map_err(|e| e.to_string())?;
    let contents = validate(&copy).map_err(|e| e.to_string())?;
    check_space(free_space, root, import_space_needed(&contents))?;
    unpack(
        &copy,
        &work.join(DUMP_ENTRY),
        &work.join(ARCHIVE_ENTRY),
        &contents,
    )
    .map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&copy);
    Ok(contents)
}

fn commit(stack: &mut Stack, work: &Path, data: &DataDir, rename: Rename) -> Result<(), String> {
    let dump = work.join(DUMP_ENTRY);
    stack.stage_import(&dump).map_err(|e| {
        format!("the backup was refused or could not be restored (nothing was changed): {e}")
    })?;
    let _ = std::fs::remove_file(&dump);
    let journal = data_root(data).join(IMPORT_JOURNAL);
    let discard = |stack: &Stack, e: String| -> String {
        let _ = remove_file_if_exists(&journal);
        match stack.discard_import() {
            Ok(()) => e,
            Err(d) => format!("{e}; dropping the staged import failed: {d}"),
        }
    };
    if let Err(e) = std::fs::write(&journal, b"import in progress\n") {
        return Err(discard(stack, format!("{}: {e}", journal.display())));
    }
    if let Err(e) = stack.stop_server(StopBudget::Full) {
        let msg = discard(stack, e.to_string());
        return Err(match stack.restart_server() {
            Ok(()) => msg,
            Err(r) => format!("{msg}; the server did not start again: {r}"),
        });
    }
    let archive = data.archive();
    let committed = match swap_dir(&work.join(ARCHIVE_ENTRY), &archive, rename) {
        // Both renames back failed: the replays are only in `archive.old`. Journal and staging
        // kept, server left stopped: the next start puts the folder back (rollback).
        Err(e) if !archive.exists() && old_dir(&archive).exists() => {
            return Err(format!(
                "the replay folder could not be replaced and the previous one could not be put                  back ({e}); the server is stopped: restart Storm Codex to finish the rollback"
            ));
        }
        Err(e) => Err(discard(
            stack,
            format!("the replay folder could not be replaced (nothing was changed): {e}"),
        )),
        Ok(()) => match stack.promote_import() {
            Ok(()) => complete_import(data).map_err(|e| {
                format!(
                    "the import is done but its cleanup failed (retried at the next start): {e}"
                )
            }),
            Err(e) => match stack.staging_exists() {
                // The rename did not happen: the previous database is still the app's.
                Ok(true) => match unswap_dir(&archive) {
                    Ok(()) => Err(discard(
                        stack,
                        format!(
                            "switching to the imported database failed (nothing was changed): {e}"
                        ),
                    )),
                    // Journal and staging kept: the next start finishes the rollback. The
                    // server stays stopped rather than run on a replay folder that does not
                    // match.
                    Err(u) => {
                        return Err(format!(
                            "switching to the imported database failed ({e}) and the previous                              replay folder could not be put back ({u}); restart Storm Codex to                              finish the rollback"
                        ))
                    }
                },
                // The rename committed: the imported database is the app's. Never roll back;
                // complete the import (journal kept on failure: the next start completes it).
                Ok(false) => stack
                    .finish_import()
                    .map_err(|f| f.to_string())
                    .and_then(|()| complete_import(data))
                    .map_err(|f| {
                        format!(
                            "the backup was imported and is in use, but its cleanup failed                              ({e}; {f}); it is completed at the next start"
                        )
                    }),
                // Unknown state: leave everything (journal included) to the next start.
                Err(s) => {
                    return Err(format!(
                        "switching to the imported database failed ({e}) and whether it took                          effect could not be read ({s}); the server is stopped: restart Storm                          Codex to complete or roll back the import"
                    ))
                }
            },
        },
    };
    let restarted = stack
        .restart_server()
        .map_err(|e| format!("the server did not start again: {e}"));
    match (committed, restarted) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(e), Ok(())) | (Ok(()), Err(e)) => Err(e),
        (Err(a), Err(b)) => Err(format!("{a}; {b}")),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::supervise::fakes::*;
    use crate::supervise::SERVER_PORT;
    use std::collections::BTreeMap;
    use zip::write::SimpleFileOptions;

    fn write(path: &Path, body: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    /// Every entry of a zip: name -> content.
    fn read_zip(path: &Path) -> BTreeMap<String, Vec<u8>> {
        let mut zip = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
        let mut out = BTreeMap::new();
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).unwrap();
            let mut body = Vec::new();
            f.read_to_end(&mut body).unwrap();
            out.insert(f.name().unwrap().to_string(), body);
        }
        out
    }

    /// A zip with the given (name, body) entries, written as is.
    fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, body) in entries {
            if name.ends_with('/') {
                zip.add_directory(*name, opts).unwrap();
            } else {
                zip.start_file(*name, opts).unwrap();
                zip.write_all(body).unwrap();
            }
        }
        zip.finish().unwrap();
    }

    fn archive_tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
        fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
            for e in std::fs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    walk(root, &p, out);
                } else {
                    let rel = p
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    out.insert(rel, std::fs::read(&p).unwrap());
                }
            }
        }
        let mut out = BTreeMap::new();
        if dir.exists() {
            walk(dir, dir, &mut out);
        }
        out
    }

    #[test]
    fn backup_name_is_the_utc_date() {
        assert_eq!(
            backup_file_name(0),
            "storm-codex-backup-1970-01-01.stormcodex"
        );
        // 2026-11-13T12:00:00Z
        assert_eq!(
            backup_file_name(1_794_571_200),
            "storm-codex-backup-2026-11-13.stormcodex"
        );
        // 2024-02-29T23:59:59Z (leap day)
        assert_eq!(
            backup_file_name(1_709_251_199),
            "storm-codex-backup-2024-02-29.stormcodex"
        );
    }

    #[test]
    fn export_zip_contains_db_dump_and_archive() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("archive");
        write(&archive.join("ab").join("abcdef.StormReplay"), b"replay-1");
        write(&archive.join("cd").join("cdef01.StormReplay"), b"replay-2");
        let out = tmp.path().join("out").join("b.stormcodex");
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        let contents = export(
            |p| std::fs::write(p, b"PGDMP-fake").map_err(|e| e.to_string()),
            &archive,
            &tmp.path().join("work"),
            &out,
        )
        .expect("export");
        assert_eq!(contents.archive_files, 2);
        assert_eq!(contents.dump_bytes, 10);
        let entries = read_zip(&out);
        assert_eq!(
            entries.get("db.dump").map(Vec::as_slice),
            Some(&b"PGDMP-fake"[..])
        );
        assert_eq!(
            entries
                .get("archive/ab/abcdef.StormReplay")
                .map(Vec::as_slice),
            Some(&b"replay-1"[..])
        );
        assert_eq!(
            entries
                .get("archive/cd/cdef01.StormReplay")
                .map(Vec::as_slice),
            Some(&b"replay-2"[..])
        );
        // Stored (no recompression), nothing left behind.
        let mut zip = zip::ZipArchive::new(File::open(&out).unwrap()).unwrap();
        for i in 0..zip.len() {
            assert_eq!(
                zip.by_index(i).unwrap().compression(),
                zip::CompressionMethod::Stored
            );
        }
        assert!(!partial_path(&out).exists());
        // A file already at the temporary name is never overwritten.
        write(&partial_path(&out), b"someone else's file");
        let again = tmp.path().join("out").join("b.stormcodex");
        assert!(export(
            |p| std::fs::write(p, b"PGDMP-fake").map_err(|e| e.to_string()),
            &archive,
            &tmp.path().join("work"),
            &again,
        )
        .is_err());
        assert_eq!(
            std::fs::read(partial_path(&out)).unwrap(),
            b"someone else's file"
        );
        assert!(validate(&out).is_ok());
    }

    #[test]
    fn export_with_a_failed_dump_writes_no_file() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("b.stormcodex");
        let err = export(
            |_| Err("pg_dump exploded".into()),
            &tmp.path().join("archive"),
            &tmp.path().join("work"),
            &out,
        )
        .expect_err("must fail");
        assert!(err.to_string().contains("pg_dump exploded"), "{err}");
        assert!(!out.exists());
    }

    fn invalid(entries: &[(&str, &[u8])]) -> String {
        let tmp = tempfile::tempdir().unwrap();
        let zip = tmp.path().join("bad.stormcodex");
        make_zip(&zip, entries);
        let err = validate(&zip).expect_err("must be rejected");
        assert!(matches!(err, BackupError::Invalid(_)), "{err:?}");
        err.to_string()
    }

    #[test]
    fn zip_slip_entries_are_rejected() {
        let dump: (&str, &[u8]) = ("db.dump", b"PGDMP");
        for evil in [
            "../evil.txt",
            "archive/../../evil.txt",
            "archive/../db.dump",
            "/etc/evil",
            "C:/Windows/evil.dll",
            "archive/C:/evil",
            "archive\\..\\..\\evil.txt",
            "archive//double",
            "archive/./x",
            "archive/CON",
            "archive/x/nul.txt",
            "archive/trailing.",
            "archive/CONIN$",
            "archive/x/conout$.txt",
            "archive/clock$.log",
            "archive/COM\u{b9}",
            "archive/lpt\u{b3}.txt",
            "archive/con .txt",
        ] {
            let msg = invalid(&[dump, (evil, b"x")]);
            assert!(msg.contains("entry"), "{evil}: {msg}");
        }
    }

    #[test]
    fn import_validation_rejects_bad_structure() {
        // No dump.
        assert!(invalid(&[("archive/a.StormReplay", b"x")]).contains("db.dump"));
        // Unknown top-level entry.
        assert!(invalid(&[("db.dump", b"PGDMP"), ("readme.txt", b"x")]).contains("readme.txt"));
        // Empty dump.
        assert!(invalid(&[("db.dump", b"")]).contains("db.dump"));
        // A dump that is not a pg_dump custom-format file.
        assert!(invalid(&[("db.dump", b"hello")]).contains("db.dump"));
        // Not a zip at all.
        let tmp = tempfile::tempdir().unwrap();
        let junk = tmp.path().join("junk.stormcodex");
        std::fs::write(&junk, b"not a zip").unwrap();
        assert!(matches!(validate(&junk), Err(BackupError::Zip { .. })));
        assert!(matches!(
            validate(&tmp.path().join("missing.stormcodex")),
            Err(BackupError::Io { .. })
        ));
    }

    #[test]
    fn symlink_entries_are_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("link.stormcodex");
        let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
        let opts = SimpleFileOptions::default();
        zip.start_file("db.dump", opts).unwrap();
        zip.write_all(b"PGDMP").unwrap();
        zip.add_symlink("archive/link", "../../outside", opts)
            .unwrap();
        zip.finish().unwrap();
        let err = validate(&path).expect_err("symlink");
        assert!(err.to_string().contains("archive/link"), "{err}");
    }

    #[test]
    fn unpack_extracts_dump_and_archive_and_refuses_an_existing_target() {
        let tmp = tempfile::tempdir().unwrap();
        let zip = tmp.path().join("ok.stormcodex");
        make_zip(
            &zip,
            &[
                ("db.dump", b"PGDMP-1"),
                ("archive/", b""),
                ("archive/ab/", b""),
                ("archive/ab/x.StormReplay", b"xx"),
            ],
        );
        let contents = validate(&zip).expect("valid");
        assert_eq!(contents.archive_files, 1);
        assert_eq!(contents.total_bytes, 9);
        let work = tmp.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        unpack(
            &zip,
            &work.join("db.dump"),
            &work.join("archive"),
            &contents,
        )
        .expect("unpack");
        assert_eq!(std::fs::read(work.join("db.dump")).unwrap(), b"PGDMP-1");
        assert_eq!(
            archive_tree(&work.join("archive")),
            BTreeMap::from([("ab/x.StormReplay".to_string(), b"xx".to_vec())])
        );
        assert!(unpack(&zip, &work.join("d2"), &work.join("archive"), &contents).is_err());
    }

    #[test]
    fn unpack_refuses_more_than_the_validated_budget() {
        let tmp = tempfile::tempdir().unwrap();
        let zip = tmp.path().join("ok.stormcodex");
        make_zip(
            &zip,
            &[
                ("db.dump", b"PGDMP-1"),
                ("archive/a.StormReplay", b"aa"),
                ("archive/b.StormReplay", b"bb"),
            ],
        );
        let full = validate(&zip).expect("valid");
        let tighter = [
            BackupContents {
                archive_files: 1,
                ..full.clone()
            },
            BackupContents {
                total_bytes: full.total_bytes - 1,
                ..full.clone()
            },
            BackupContents {
                dump_bytes: 5,
                ..full.clone()
            },
        ];
        for (i, budget) in tighter.iter().enumerate() {
            let out = tmp.path().join(format!("w{i}"));
            let err = unpack(&zip, &out.join("db.dump"), &out.join("archive"), budget)
                .expect_err("over budget");
            assert!(
                matches!(err, BackupError::Invalid(_)),
                "{budget:?}: {err:?}"
            );
        }
        let out = tmp.path().join("ok");
        unpack(&zip, &out.join("db.dump"), &out.join("archive"), &full).expect("exact budget");
    }

    #[test]
    fn network_device_and_relative_paths_are_refused() {
        for bad in [
            r"\\server\share\b.stormcodex",
            "//server/share/b.stormcodex",
            r"\\?\C:\b.stormcodex",
            r"\\.\C:\b.stormcodex",
            r"\\?\UNC\server\share\b.stormcodex",
            "b.stormcodex",
            r"..\b.stormcodex",
        ] {
            assert!(check_local_path(Path::new(bad)).is_err(), "{bad}");
        }
        let tmp = tempfile::tempdir().unwrap();
        check_local_path(&tmp.path().join("b.stormcodex")).expect("local temp folder");
    }

    #[test]
    fn private_copy_refuses_a_file_that_changed() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src.stormcodex");
        std::fs::write(&src, b"0123456789").unwrap();
        copy_private(&src, &tmp.path().join("a"), 10).expect("same size");
        assert_eq!(std::fs::read(tmp.path().join("a")).unwrap(), b"0123456789");
        // Grew or shrank since its size was read.
        assert!(copy_private(&src, &tmp.path().join("b"), 8).is_err());
        assert!(copy_private(&src, &tmp.path().join("c"), 12).is_err());
        // Never overwrites.
        assert!(copy_private(&src, &tmp.path().join("a"), 10).is_err());
    }

    #[test]
    fn import_space_counts_unpacked_data_staging_database_and_margin() {
        let c = BackupContents {
            dump_bytes: 100,
            archive_files: 3,
            total_bytes: 1_000,
        };
        assert_eq!(import_space_needed(&c), 1_000 + 300 + IMPORT_FREE_MARGIN);
    }

    #[test]
    fn swap_dir_puts_the_target_back_when_the_second_rename_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("archive");
        let new = tmp.path().join("import-work").join("archive");
        write(&target.join("old.StormReplay"), b"old");
        write(&new.join("new.StormReplay"), b"new");
        let calls = std::cell::Cell::new(0);
        let err = swap_dir(&new, &target, |a, b| {
            calls.set(calls.get() + 1);
            if calls.get() == 2 {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "locked"))
            } else {
                std::fs::rename(a, b)
            }
        })
        .expect_err("second rename fails");
        assert!(err.to_string().contains("locked"), "{err}");
        assert_eq!(
            archive_tree(&target),
            BTreeMap::from([("old.StormReplay".to_string(), b"old".to_vec())])
        );
        assert!(new.exists());
        assert!(!tmp.path().join("archive.old").exists());
        // A successful swap keeps the old folder until the import is committed; unswap puts
        // it back.
        swap_dir(&new, &target, |a, b| std::fs::rename(a, b)).expect("swap");
        assert!(archive_tree(&target).contains_key("new.StormReplay"));
        assert!(tmp.path().join("archive.old").exists());
        unswap_dir(&target).expect("unswap");
        assert_eq!(
            archive_tree(&target),
            BTreeMap::from([("old.StormReplay".to_string(), b"old".to_vec())])
        );
        assert!(!tmp.path().join("archive.old").exists());
        assert!(has_files(&target));
        assert!(!has_files(&tmp.path().join("nothing")));
        std::fs::create_dir_all(tmp.path().join("empty").join("sub")).unwrap();
        assert!(!has_files(&tmp.path().join("empty")));
    }

    fn stack_with_archive(
        host: FakeHost,
        root: &Path,
        replay: Option<&[u8]>,
    ) -> (Stack, std::sync::Arc<FakeHost>) {
        let (stack, host) = start(host, root);
        let stack = stack.expect("start");
        if let Some(body) = replay {
            write(
                &stack.data().archive().join("aa").join("old.StormReplay"),
                body,
            );
        }
        write(&stack.data().ledger(), b"{\"ledger\":1}");
        (stack, host)
    }

    fn good_backup(dir: &Path) -> PathBuf {
        let zip = dir.join("in.stormcodex");
        make_zip(
            &zip,
            &[
                ("db.dump", b"PGDMP-import"),
                ("archive/bb/new.StormReplay", b"new"),
            ],
        );
        zip
    }

    fn plenty(_: &Path) -> io::Result<u64> {
        Ok(u64::MAX)
    }

    fn old_archive() -> BTreeMap<String, Vec<u8>> {
        BTreeMap::from([("aa/old.StormReplay".to_string(), b"old".to_vec())])
    }

    fn new_archive() -> BTreeMap<String, Vec<u8>> {
        BTreeMap::from([("bb/new.StormReplay".to_string(), b"new".to_vec())])
    }

    /// No work folder, journal or `archive.old` left behind.
    fn assert_clean(root: &Path) {
        for leftover in [IMPORT_WORK, IMPORT_JOURNAL, "archive.old"] {
            assert!(!root.join(leftover).exists(), "{leftover} left behind");
        }
    }

    #[test]
    fn import_stages_then_swaps_promotes_resets_the_ledger_and_restarts() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        let log = Log::default();
        let (mut stack, host) = stack_with_archive(FakeHost::new(&log), tmp.path(), Some(b"old"));
        log.lock().unwrap().clear();
        let zip = good_backup(src.path());
        import_into(&mut stack, &zip, plenty).expect("import");
        assert_eq!(
            entries(&log),
            [
                "db.stage(PGDMP-import)".to_string(),
                "server.migrate".to_string(),
                format!("server.shutdown({SERVER_PORT})"),
                "db.promote".to_string(),
                "server.spawn".to_string(),
            ]
        );
        assert_eq!(archive_tree(&stack.data().archive()), new_archive());
        assert!(!stack.data().ledger().exists(), "the ledger is reset");
        assert!(!host.staged.load(std::sync::atomic::Ordering::SeqCst));
        assert_clean(tmp.path());
        // The chosen file is untouched.
        assert!(zip.exists());
    }

    #[test]
    fn refused_stage_changes_nothing_and_keeps_the_server_up() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        let log = Log::default();
        let mut host = FakeHost::new(&log);
        host.fail_stage = true;
        let (mut stack, _host) = stack_with_archive(host, tmp.path(), Some(b"old"));
        log.lock().unwrap().clear();
        let err = import_into(&mut stack, &good_backup(src.path()), plenty).expect_err("refused");
        assert!(err.contains("fake failure"), "{err}");
        assert_eq!(entries(&log), ["db.stage(PGDMP-import)", "server.migrate"]);
        assert_eq!(archive_tree(&stack.data().archive()), old_archive());
        assert!(stack.data().ledger().exists());
        assert_clean(tmp.path());
    }

    #[test]
    fn failed_promote_puts_the_archive_back_drops_the_stage_and_restarts() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        let log = Log::default();
        let mut host = FakeHost::new(&log);
        host.fail_promote = true;
        let (mut stack, _host) = stack_with_archive(host, tmp.path(), Some(b"old"));
        log.lock().unwrap().clear();
        let err =
            import_into(&mut stack, &good_backup(src.path()), plenty).expect_err("promote fails");
        assert!(err.contains("nothing was changed"), "{err}");
        assert_eq!(
            entries(&log),
            [
                "db.stage(PGDMP-import)".to_string(),
                "server.migrate".to_string(),
                format!("server.shutdown({SERVER_PORT})"),
                "db.promote".to_string(),
                "db.discard".to_string(),
                "server.spawn".to_string(),
            ]
        );
        assert_eq!(archive_tree(&stack.data().archive()), old_archive());
        assert!(stack.data().ledger().exists());
        assert_clean(tmp.path());
    }

    #[test]
    fn a_backup_from_a_newer_app_is_refused_with_the_migrate_message() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        let log = Log::default();
        let mut host = FakeHost::new(&log);
        host.fail_migrate = Some("made by a newer version of Storm Codex".into());
        let (mut stack, _host) = stack_with_archive(host, tmp.path(), Some(b"old"));
        log.lock().unwrap().clear();
        let err = import_into(&mut stack, &good_backup(src.path()), plenty).expect_err("newer");
        assert!(err.contains("newer version"), "{err}");
        assert!(err.contains("nothing was changed"), "{err}");
        assert_eq!(entries(&log), ["db.stage(PGDMP-import)", "server.migrate"]);
        assert_eq!(archive_tree(&stack.data().archive()), old_archive());
        assert_clean(tmp.path());
    }

    /// The rename committed, then the promote's cleanup failed: the import is completed
    /// (never rolled back) and reported as done.
    #[test]
    fn a_failure_after_the_rename_committed_rolls_forward() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        let log = Log::default();
        let mut host = FakeHost::new(&log);
        host.fail_promote_after_rename = true;
        let (mut stack, host) = stack_with_archive(host, tmp.path(), Some(b"old"));
        log.lock().unwrap().clear();
        import_into(&mut stack, &good_backup(src.path()), plenty).expect("rolled forward");
        assert_eq!(
            entries(&log),
            [
                "db.stage(PGDMP-import)".to_string(),
                "server.migrate".to_string(),
                format!("server.shutdown({SERVER_PORT})"),
                "db.promote".to_string(),
                "db.finish".to_string(),
                "server.spawn".to_string(),
            ]
        );
        assert_eq!(archive_tree(&stack.data().archive()), new_archive());
        assert!(!stack.data().ledger().exists(), "the ledger is reset");
        assert!(!host.staged.load(std::sync::atomic::Ordering::SeqCst));
        assert_clean(tmp.path());
    }

    /// Both renames of the swap fail and the previous folder cannot be put back: journal and
    /// staging kept, server left stopped; the next start rolls back.
    #[test]
    fn a_double_swap_failure_leaves_the_server_stopped_for_the_next_start() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        let log = Log::default();
        let (mut stack, host) = stack_with_archive(FakeHost::new(&log), tmp.path(), Some(b"old"));
        log.lock().unwrap().clear();
        let calls = std::cell::Cell::new(0);
        let rename = |a: &Path, b: &Path| {
            calls.set(calls.get() + 1);
            if calls.get() == 1 {
                std::fs::rename(a, b)
            } else {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "locked"))
            }
        };
        let err = import_into_with(&mut stack, &good_backup(src.path()), plenty, &rename)
            .expect_err("double failure");
        assert!(err.contains("restart Storm Codex"), "{err}");
        assert!(!err.contains("nothing was changed"), "{err}");
        assert_eq!(
            entries(&log),
            [
                "db.stage(PGDMP-import)".to_string(),
                "server.migrate".to_string(),
                format!("server.shutdown({SERVER_PORT})"),
            ]
        );
        assert!(tmp.path().join(IMPORT_JOURNAL).exists());
        assert!(host.staged.load(std::sync::atomic::Ordering::SeqCst));
        assert!(!stack.data().archive().exists());
        assert_eq!(archive_tree(&tmp.path().join("archive.old")), old_archive());
        // The next start rolls back.
        drop(stack);
        let log2 = Log::default();
        let host2 = FakeHost::new(&log2);
        host2
            .staged
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let (stack2, _host2) = start(host2, tmp.path());
        let stack2 = stack2.expect("start");
        assert_eq!(recovery_steps(&log2), ["db.discard"]);
        assert_eq!(archive_tree(&stack2.data().archive()), old_archive());
        assert_clean(tmp.path());
    }

    #[test]
    fn a_leftover_archive_old_blocks_the_swap_and_is_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        let log = Log::default();
        let (mut stack, _host) = stack_with_archive(FakeHost::new(&log), tmp.path(), Some(b"old"));
        write(
            &tmp.path().join("archive.old").join("keep.StormReplay"),
            b"keep",
        );
        log.lock().unwrap().clear();
        let err = import_into(&mut stack, &good_backup(src.path()), plenty).expect_err("blocked");
        assert!(err.contains("could not be replaced"), "{err}");
        assert_eq!(
            entries(&log),
            [
                "db.stage(PGDMP-import)".to_string(),
                "server.migrate".to_string(),
                format!("server.shutdown({SERVER_PORT})"),
                "db.discard".to_string(),
                "server.spawn".to_string(),
            ]
        );
        assert_eq!(archive_tree(&stack.data().archive()), old_archive());
        assert_eq!(
            archive_tree(&tmp.path().join("archive.old")),
            BTreeMap::from([("keep.StormReplay".to_string(), b"keep".to_vec())])
        );
        assert!(!tmp.path().join(IMPORT_JOURNAL).exists());
    }

    #[test]
    fn invalid_backup_network_path_or_low_disk_touches_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        let log = Log::default();
        let (mut stack, _host) = stack_with_archive(FakeHost::new(&log), tmp.path(), Some(b"old"));
        log.lock().unwrap().clear();
        let bad = src.path().join("bad.stormcodex");
        make_zip(&bad, &[("db.dump", b"PGDMP"), ("../evil", b"x")]);
        let err = import_into(&mut stack, &bad, plenty).expect_err("invalid");
        assert!(err.contains("not a valid Storm Codex backup"), "{err}");
        let err =
            import_into(&mut stack, &good_backup(src.path()), |_| Ok(1024)).expect_err("disk");
        assert!(err.contains("free"), "{err}");
        let err = import_into(
            &mut stack,
            Path::new(r"\\server\share\in.stormcodex"),
            plenty,
        )
        .expect_err("network");
        assert!(err.contains("network"), "{err}");
        assert!(entries(&log).is_empty(), "{:?}", entries(&log));
        assert_eq!(archive_tree(&stack.data().archive()), old_archive());
        assert!(stack.data().ledger().exists());
        assert!(!src.path().join("..").join("evil").exists());
        assert_clean(tmp.path());
    }

    /// Lays out the data root as a crash would leave it, then starts the stack.
    fn crashed(
        root: &Path,
        journal: bool,
        staged: bool,
        archive: Option<&[u8]>,
        archive_old: Option<&[u8]>,
    ) -> (Stack, Log) {
        let data = DataDir::new(root.to_path_buf());
        if let Some(body) = archive {
            write(&data.archive().join("bb").join("new.StormReplay"), body);
        }
        if let Some(body) = archive_old {
            write(
                &root.join("archive.old").join("aa").join("old.StormReplay"),
                body,
            );
        }
        if journal {
            write(&root.join(IMPORT_JOURNAL), b"import in progress\n");
        }
        write(&data.ledger(), b"{}");
        write(&root.join(IMPORT_WORK).join("db.dump"), b"PGDMP");
        let log = Log::default();
        let host = FakeHost::new(&log);
        host.staged
            .store(staged, std::sync::atomic::Ordering::SeqCst);
        let (stack, _host) = start(host, root);
        (stack.expect("start"), log)
    }

    /// The recovery steps logged before the server started.
    fn recovery_steps(log: &Log) -> Vec<String> {
        let all = entries(log);
        let first = all
            .iter()
            .position(|e| e.starts_with("db.ensure_database"))
            .unwrap();
        let spawn = all.iter().position(|e| e == "server.spawn").unwrap();
        all[first + 1..spawn]
            .iter()
            .filter(|e| !e.starts_with("pick_port"))
            .cloned()
            .collect()
    }

    #[test]
    fn crash_after_the_swap_before_the_promote_is_rolled_back_at_start() {
        let tmp = tempfile::tempdir().unwrap();
        let (stack, log) = crashed(tmp.path(), true, true, Some(b"new"), Some(b"old"));
        assert_eq!(recovery_steps(&log), ["db.discard"]);
        assert_eq!(archive_tree(&stack.data().archive()), old_archive());
        assert!(stack.data().ledger().exists());
        assert_clean(tmp.path());
    }

    #[test]
    fn crash_in_the_middle_of_the_swap_is_rolled_back_at_start() {
        let tmp = tempfile::tempdir().unwrap();
        let (stack, log) = crashed(tmp.path(), true, true, None, Some(b"old"));
        assert_eq!(recovery_steps(&log), ["db.discard"]);
        assert_eq!(archive_tree(&stack.data().archive()), old_archive());
        assert_clean(tmp.path());
    }

    #[test]
    fn crash_before_the_swap_keeps_the_archive() {
        let tmp = tempfile::tempdir().unwrap();
        let (stack, log) = crashed(tmp.path(), true, true, Some(b"new"), None);
        assert_eq!(recovery_steps(&log), ["db.discard"]);
        assert_eq!(archive_tree(&stack.data().archive()), new_archive());
        assert_clean(tmp.path());
    }

    #[test]
    fn crash_after_the_promote_is_completed_at_start() {
        let tmp = tempfile::tempdir().unwrap();
        let (stack, log) = crashed(tmp.path(), true, false, Some(b"new"), Some(b"old"));
        assert_eq!(recovery_steps(&log), ["db.finish"]);
        assert_eq!(archive_tree(&stack.data().archive()), new_archive());
        assert!(!stack.data().ledger().exists(), "the ledger is reset");
        assert_clean(tmp.path());
    }

    #[test]
    fn without_a_journal_an_orphan_stage_is_dropped_and_archives_are_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let (stack, log) = crashed(tmp.path(), false, true, Some(b"new"), Some(b"old"));
        assert_eq!(recovery_steps(&log), ["db.discard"]);
        // Both folders hold files and no journal says which is current: nothing is deleted.
        assert_eq!(archive_tree(&stack.data().archive()), new_archive());
        assert!(tmp.path().join("archive.old").exists());
        assert!(stack.data().ledger().exists());
        assert!(!tmp.path().join(IMPORT_WORK).exists());
        // An empty archive next to `archive.old` gets it back.
        let tmp2 = tempfile::tempdir().unwrap();
        let (stack2, log2) = crashed(tmp2.path(), false, false, None, Some(b"old"));
        assert!(recovery_steps(&log2).is_empty());
        assert_eq!(archive_tree(&stack2.data().archive()), old_archive());
        assert_clean(tmp2.path());
    }

    /// Real Postgres 17: a row and a replay exported from one data root come back in a fresh
    /// one (data-only dump restored into a schema built by the migrations). The server's
    /// `migrate` subcommand is played by psql over the server's migration files (same ledger
    /// table, deterministic checksum). Ignored by default; run it with `PG_BIN_DIR` pointing at
    /// Postgres 17's `bin/` and `cargo test -- --ignored` (it fails, never skips, without it).
    #[test]
    #[ignore = "needs PG_BIN_DIR (Postgres 17 bin/); run with --ignored"]
    fn export_then_import_roundtrip_preserves_a_settings_row() {
        use storm_codex_supervisor::{Postgres, APP_DATABASE, MIGRATIONS_TABLE};
        let bin = std::env::var_os("PG_BIN_DIR")
            .filter(|v| !v.is_empty())
            .expect("PG_BIN_DIR must point at Postgres 17's bin/ for this test");
        let bin = PathBuf::from(bin);
        let free_port = || {
            std::net::TcpListener::bind("127.0.0.1:0")
                .unwrap()
                .local_addr()
                .unwrap()
                .port()
        };
        struct Running(Postgres);
        impl Drop for Running {
            fn drop(&mut self) {
                let _ = self.0.stop();
            }
        }
        let run_psql = |url: &str, args: &[&std::ffi::OsStr]| -> Result<String, String> {
            let out = std::process::Command::new(bin.join("psql"))
                .args([
                    "-X",
                    "-w",
                    "-q",
                    "-t",
                    "-A",
                    "-v",
                    "ON_ERROR_STOP=1",
                    "-d",
                    url,
                ])
                .args(args)
                .env_remove("PGPASSWORD")
                .env("PGCLIENTENCODING", "UTF8")
                .output()
                .map_err(|e| e.to_string())?;
            if !out.status.success() {
                return Err(String::from_utf8_lossy(&out.stderr).into_owned());
            }
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
        };
        let sql = |url: &str, q: &str| run_psql(url, &["-c".as_ref(), q.as_ref()]);
        let migrations = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("crates")
            .join("storm-codex-server")
            .join("migrations");
        let mut files: Vec<(i64, PathBuf)> = std::fs::read_dir(&migrations)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "sql"))
            .map(|p| {
                let name = p.file_name().unwrap().to_string_lossy().to_string();
                let digits: String = name.chars().take_while(char::is_ascii_digit).collect();
                (digits.parse().unwrap(), p)
            })
            .collect();
        files.sort();
        let migrate = |url: &str, target: Option<i64>| -> Result<(), String> {
            sql(
                url,
                &format!(
                    "CREATE TABLE IF NOT EXISTS {MIGRATIONS_TABLE} (version BIGINT PRIMARY KEY, \
                     description TEXT NOT NULL, installed_on TIMESTAMPTZ NOT NULL DEFAULT now(), \
                     success BOOLEAN NOT NULL, checksum BYTEA NOT NULL, \
                     execution_time BIGINT NOT NULL)"
                ),
            )?;
            let applied = sql(url, &format!("SELECT version FROM {MIGRATIONS_TABLE}"))?;
            let applied: Vec<i64> = applied.lines().filter_map(|l| l.parse().ok()).collect();
            for (v, file) in &files {
                if target.is_some_and(|t| *v > t) || applied.contains(v) {
                    continue;
                }
                run_psql(url, &["-f".as_ref(), file.as_os_str()])?;
                sql(
                    url,
                    &format!(
                        "INSERT INTO {MIGRATIONS_TABLE} (version, description, success, \
                         checksum, execution_time) \
                         VALUES ({v}, 'm{v}', true, decode(md5('m{v}'), 'hex'), 1)"
                    ),
                )?;
            }
            Ok(())
        };
        let boot = |root: &Path| -> Running {
            let data = DataDir::new(root.to_path_buf());
            data.ensure().unwrap();
            let pg = Postgres::new(bin.clone(), data.pg(), free_port());
            pg.init_if_needed().unwrap();
            pg.start().unwrap();
            pg.ensure_database(APP_DATABASE).unwrap();
            migrate(&pg.database_url(APP_DATABASE), None).unwrap();
            Running(pg)
        };
        let marker = "SELECT value FROM app_settings WHERE key = 'marker'";

        // Source PC.
        let a = tempfile::tempdir().unwrap();
        let src = boot(a.path());
        let src_url = src.0.database_url(APP_DATABASE);
        sql(
            &src_url,
            "INSERT INTO app_settings VALUES ('marker', '\"zoe\"')",
        )
        .unwrap();
        let data_a = DataDir::new(a.path().to_path_buf());
        write(
            &data_a.archive().join("ab").join("abc.StormReplay"),
            b"replay-bytes",
        );
        let out = a.path().join("backup.stormcodex");
        export(
            |p| src.0.dump(p).map_err(|e| e.to_string()),
            &data_a.archive(),
            &a.path().join("export-work"),
            &out,
        )
        .expect("export");
        drop(src);

        // New PC: fresh cluster, then validate, unpack, stage, swap, promote.
        let b = tempfile::tempdir().unwrap();
        let dst = boot(b.path());
        let dst_url = dst.0.database_url(APP_DATABASE);
        let contents = validate(&out).expect("valid");
        assert_eq!(contents.archive_files, 1);
        let work = b.path().join("import-work");
        std::fs::create_dir_all(&work).unwrap();
        unpack(
            &out,
            &work.join("db.dump"),
            &work.join("archive"),
            &contents,
        )
        .expect("unpack");
        dst.0
            .stage_import(&work.join("db.dump"), &migrate)
            .expect("stage");
        let data_b = DataDir::new(b.path().to_path_buf());
        swap_dir(&work.join("archive"), &data_b.archive(), rename_retrying).expect("swap");
        dst.0.promote_import().expect("promote");
        assert_eq!(sql(&dst_url, marker).unwrap(), "\"zoe\"");
        assert_eq!(
            std::fs::read(data_b.archive().join("ab").join("abc.StormReplay")).unwrap(),
            b"replay-bytes"
        );
        // Importing again replaces the whole database: still one marker row.
        dst.0
            .stage_import(&work.join("db.dump"), &migrate)
            .expect("stage again");
        dst.0.promote_import().expect("promote again");
        assert_eq!(
            sql(
                &dst_url,
                "SELECT count(*) FROM app_settings WHERE key = 'marker'"
            )
            .unwrap(),
            "1"
        );
    }
}
