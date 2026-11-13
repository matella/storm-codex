//! Backup archives (`.stormcodex`): a zip holding `db.dump` (`pg_dump -Fc` of the app
//! database) and the whole `archive/` folder (the raw replays, the source of truth).
//!
//! Export streams every file into the zip (`Stored`: replays and the custom-format dump are
//! already compressed), never loading a file in memory. Import validates the whole zip before
//! touching anything (entry names, kinds, sizes), unpacks it next to the data, then stops the
//! server, restores the dump and swaps the archive folder in.

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
/// the zip is built in `<out>.partial` and renamed to `out` once complete. The dump is taken
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
    let result = write_zip(&partial, &dump_file, archive_dir, &files);
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
    partial: &Path,
    dump_file: &Path,
    archive_dir: &Path,
    files: &[String],
) -> Result<BackupContents, BackupError> {
    let zip_err = |source| BackupError::Zip {
        path: partial.to_path_buf(),
        source,
    };
    let file = File::create(partial).map_err(io_err(partial))?;
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
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
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
        let stem = part.split('.').next().unwrap_or(part).to_ascii_lowercase();
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
/// (which must not exist yet). Names are re-checked; files are created new (never
/// overwritten).
pub fn unpack(file: &Path, dump_out: &Path, archive_out: &Path) -> Result<(), BackupError> {
    if archive_out.exists() {
        return Err(io_err(archive_out)(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "target folder already exists",
        )));
    }
    std::fs::create_dir_all(archive_out).map_err(io_err(archive_out))?;
    let mut zip = open_zip(file)?;
    let zip_err = |source| BackupError::Zip {
        path: file.to_path_buf(),
        source,
    };
    for i in 0..zip.len() {
        let entry = zip.by_index(i).map_err(zip_err)?;
        let name = entry.name().map_err(zip_err)?.into_owned();
        let size = entry.size();
        let target = match classify(&name).map_err(BackupError::Invalid)? {
            Entry::Dump => dump_out.to_path_buf(),
            Entry::ArchiveDir(parts) => {
                let dir = parts
                    .iter()
                    .fold(archive_out.to_path_buf(), |p, c| p.join(c));
                std::fs::create_dir_all(&dir).map_err(io_err(&dir))?;
                continue;
            }
            Entry::ArchiveFile(parts) => parts
                .iter()
                .fold(archive_out.to_path_buf(), |p, c| p.join(c)),
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

/// Replaces folder `target` by `new` (renames on the same drive; `target` is kept as
/// `<target>.old` until `new` is in place, and put back if that fails).
pub fn replace_dir(new: &Path, target: &Path) -> Result<(), BackupError> {
    let mut old_name = target.file_name().unwrap_or_default().to_os_string();
    old_name.push(".old");
    let old = target.with_file_name(old_name);
    if old.exists() {
        std::fs::remove_dir_all(&old).map_err(io_err(&old))?;
    }
    let had_target = target.exists();
    if had_target {
        std::fs::rename(target, &old).map_err(io_err(target))?;
    }
    if let Err(e) = std::fs::rename(new, target) {
        if had_target {
            let _ = std::fs::rename(&old, target);
        }
        return Err(io_err(target)(e));
    }
    if had_target {
        // The new data is in place; a leftover `.old` is removed by the next import.
        let _ = std::fs::remove_dir_all(&old);
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

/// Imports a backup into the running stack. Refused when the app already has replays unless
/// `confirm`. The zip is validated and unpacked first (nothing changed on failure); then the
/// server is stopped (Postgres stays up), the dump restored (one transaction), the archive
/// folder swapped, and the server restarted; returns once it is healthy again.
pub fn import_into(
    stack: &mut Stack,
    file: &Path,
    confirm: bool,
    free_space: impl Fn(&Path) -> io::Result<u64>,
) -> Result<BackupContents, String> {
    let data = stack.data().clone();
    if !confirm && has_files(&data.archive()) {
        return Err(
            "this app already has replays: importing replaces all of its data; call again with \
             confirm to proceed"
                .into(),
        );
    }
    let contents = validate(file).map_err(|e| e.to_string())?;
    let root = data_root(&data);
    let free = free_space(&root).map_err(|e| format!("cannot read the free space: {e}"))?;
    let needed = contents.total_bytes.saturating_add(IMPORT_FREE_MARGIN);
    if free < needed {
        return Err(format!(
            "not enough free disk space: {needed} bytes needed, {free} bytes free"
        ));
    }
    let work = data_root(&data).join("import-work");
    if work.exists() {
        std::fs::remove_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    }
    let result = import_unpacked(stack, file, &work, &data);
    let _ = std::fs::remove_dir_all(&work);
    result.map(|()| contents)
}

fn import_unpacked(
    stack: &mut Stack,
    file: &Path,
    work: &Path,
    data: &DataDir,
) -> Result<(), String> {
    let dump = work.join(DUMP_ENTRY);
    let archive = work.join(ARCHIVE_ENTRY);
    unpack(file, &dump, &archive).map_err(|e| e.to_string())?;
    // `pg_restore --clean` takes ACCESS EXCLUSIVE locks: the server goes first.
    stack
        .stop_server(StopBudget::Full)
        .map_err(|e| e.to_string())?;
    let imported = stack
        .restore(&dump)
        .map_err(|e| format!("restoring the database failed (nothing was changed): {e}"))
        .and_then(|()| {
            replace_dir(&archive, &data.archive()).map_err(|e| {
                format!(
                    "the database was restored but the replay folder could not be replaced: {e}"
                )
            })
        });
    let restarted = stack
        .restart_server()
        .map_err(|e| format!("the server did not start again: {e}"));
    match (imported, restarted) {
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
        assert!(!out.with_extension("stormcodex.partial").exists());
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
        unpack(&zip, &work.join("db.dump"), &work.join("archive")).expect("unpack");
        assert_eq!(std::fs::read(work.join("db.dump")).unwrap(), b"PGDMP-1");
        assert_eq!(
            archive_tree(&work.join("archive")),
            BTreeMap::from([("ab/x.StormReplay".to_string(), b"xx".to_vec())])
        );
        assert!(unpack(&zip, &work.join("d2"), &work.join("archive")).is_err());
    }

    #[test]
    fn replace_dir_swaps_and_removes_the_old_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("archive");
        let new = tmp.path().join("import-work").join("archive");
        write(&target.join("old.StormReplay"), b"old");
        write(&new.join("new.StormReplay"), b"new");
        replace_dir(&new, &target).expect("replace");
        assert_eq!(
            archive_tree(&target),
            BTreeMap::from([("new.StormReplay".to_string(), b"new".to_vec())])
        );
        assert!(!new.exists());
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

    #[test]
    fn import_stops_server_restores_swaps_archive_and_restarts() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        let log = Log::default();
        let (mut stack, _host) = stack_with_archive(FakeHost::new(&log), tmp.path(), Some(b"old"));
        log.lock().unwrap().clear();
        let zip = good_backup(src.path());
        import_into(&mut stack, &zip, true, plenty).expect("import");
        assert_eq!(
            entries(&log),
            [
                format!("server.shutdown({SERVER_PORT})"),
                "db.restore(PGDMP-import)".to_string(),
                "server.spawn".to_string(),
            ]
        );
        assert_eq!(
            archive_tree(&stack.data().archive()),
            BTreeMap::from([("bb/new.StormReplay".to_string(), b"new".to_vec())])
        );
        // No work folder left next to the data.
        assert!(!tmp.path().join("import-work").exists());
        assert!(!tmp.path().join("archive.old").exists());
    }

    #[test]
    fn import_into_an_app_with_replays_needs_confirm() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        let log = Log::default();
        let (mut stack, _host) = stack_with_archive(FakeHost::new(&log), tmp.path(), Some(b"old"));
        log.lock().unwrap().clear();
        let err =
            import_into(&mut stack, &good_backup(src.path()), false, plenty).expect_err("refused");
        assert!(err.contains("confirm"), "{err}");
        assert!(entries(&log).is_empty(), "{:?}", entries(&log));
        // An app with no replay yet imports without confirm.
        let tmp2 = tempfile::tempdir().unwrap();
        let log2 = Log::default();
        let (mut fresh, _h) = stack_with_archive(FakeHost::new(&log2), tmp2.path(), None);
        import_into(&mut fresh, &good_backup(src.path()), false, plenty).expect("import");
    }

    #[test]
    fn invalid_backup_or_low_disk_touches_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        let log = Log::default();
        let (mut stack, _host) = stack_with_archive(FakeHost::new(&log), tmp.path(), Some(b"old"));
        log.lock().unwrap().clear();
        let bad = src.path().join("bad.stormcodex");
        make_zip(&bad, &[("db.dump", b"PGDMP"), ("../evil", b"x")]);
        let err = import_into(&mut stack, &bad, true, plenty).expect_err("invalid");
        assert!(err.contains("not a valid Storm Codex backup"), "{err}");
        let err = import_into(&mut stack, &good_backup(src.path()), true, |_| Ok(1024))
            .expect_err("disk");
        assert!(err.contains("free"), "{err}");
        assert!(entries(&log).is_empty(), "{:?}", entries(&log));
        assert!(has_files(&stack.data().archive()));
        assert!(!src.path().join("..").join("evil").exists());
    }

    #[test]
    fn failed_restore_keeps_the_archive_and_restarts_the_server() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        let log = Log::default();
        let mut host = FakeHost::new(&log);
        host.fail_restore = true;
        let (mut stack, _host) = stack_with_archive(host, tmp.path(), Some(b"old"));
        log.lock().unwrap().clear();
        let err = import_into(&mut stack, &good_backup(src.path()), true, plenty)
            .expect_err("restore fails");
        assert!(err.contains("fake failure"), "{err}");
        assert_eq!(
            entries(&log),
            [
                format!("server.shutdown({SERVER_PORT})"),
                "db.restore(PGDMP-import)".to_string(),
                "server.spawn".to_string(),
            ]
        );
        assert_eq!(
            archive_tree(&stack.data().archive()),
            BTreeMap::from([("aa/old.StormReplay".to_string(), b"old".to_vec())])
        );
        assert!(!tmp.path().join("import-work").exists());
    }

    /// Real Postgres 17 (skipped unless `PG_BIN_DIR` points at its `bin/`): a row and a replay
    /// exported from one data root come back in a fresh one.
    #[test]
    fn export_then_import_roundtrip_preserves_a_cast_row() {
        use storm_codex_supervisor::{Postgres, APP_DATABASE};
        let Some(bin) = std::env::var_os("PG_BIN_DIR").filter(|v| !v.is_empty()) else {
            eprintln!("PG_BIN_DIR not set: Postgres roundtrip skipped");
            return;
        };
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
        let psql = |pg: &Postgres, sql: &str| -> String {
            let out = std::process::Command::new(bin.join("psql"))
                .args(["-X", "-w", "-q", "-t", "-A", "-v", "ON_ERROR_STOP=1", "-d"])
                .arg(pg.database_url(APP_DATABASE))
                .args(["-c", sql])
                .env_remove("PGPASSWORD")
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        let boot = |root: &Path| -> Running {
            let data = DataDir::new(root.to_path_buf());
            data.ensure().unwrap();
            let pg = Postgres::new(bin.clone(), data.pg(), free_port());
            pg.init_if_needed().unwrap();
            pg.start().unwrap();
            pg.ensure_database(APP_DATABASE).unwrap();
            Running(pg)
        };

        // Source PC.
        let a = tempfile::tempdir().unwrap();
        let src = boot(a.path());
        psql(
            &src.0,
            "CREATE TABLE cast_sessions (id int PRIMARY KEY, caster text NOT NULL);              INSERT INTO cast_sessions VALUES (42, 'Zoe');",
        );
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

        // New PC: fresh cluster, then validate, unpack, restore, swap.
        let b = tempfile::tempdir().unwrap();
        let dst = boot(b.path());
        let contents = validate(&out).expect("valid");
        assert_eq!(contents.archive_files, 1);
        let work = b.path().join("import-work");
        std::fs::create_dir_all(&work).unwrap();
        unpack(&out, &work.join("db.dump"), &work.join("archive")).expect("unpack");
        dst.0.restore(&work.join("db.dump")).expect("restore");
        let data_b = DataDir::new(b.path().to_path_buf());
        replace_dir(&work.join("archive"), &data_b.archive()).expect("swap");
        assert_eq!(
            psql(&dst.0, "SELECT caster FROM cast_sessions WHERE id = 42"),
            "Zoe"
        );
        assert_eq!(
            std::fs::read(data_b.archive().join("ab").join("abc.StormReplay")).unwrap(),
            b"replay-bytes"
        );
        // Importing again over existing data replaces it (--clean), still one row.
        dst.0.restore(&work.join("db.dump")).expect("restore again");
        assert_eq!(psql(&dst.0, "SELECT count(*) FROM cast_sessions"), "1");
    }
}
