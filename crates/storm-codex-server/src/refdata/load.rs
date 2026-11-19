//! Shared dataset loading and writing, plus the box refresh and legacy bundle tooling.
//!
//! - **Box** ([`refresh`]): reads `manifest.json` from `REFDATA_URL` (a release download base), refuses a
//!   schema other than v1, skips the round when `datasetVersion` is the one last ingested, else downloads
//!   the data files, checks each against the manifest SHA-256 and writes them in one transaction. Images
//!   missing from `images_dir` come from `dataset.tar.gz` (checked against `dataset.tar.gz.sha256`).
//! - **Standalone** stages the feed or a legacy trimmed bundle ([`trim`]) via `standalone`.
//!
//! Any refusal (schema, hash, shape, resource budget) happens before the transaction: the previous
//! data is kept. Archive downloads and decompression are bounded by [`Limits`].

use super::{
    build, parse_manifest, sha256_hex, Dataset, ImageCopy, Manifest, RefdataError, Rows, DATA_FILES,
};
use sqlx::{PgPool, Postgres, Transaction};
use std::cell::Cell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::Read;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

/// `app_settings` key holding the `datasetVersion` last ingested.
pub const VERSION_KEY: &str = "refdata_dataset_version";

/// Default `REFDATA_URL`: the producer's latest validated release.
pub const DEFAULT_URL: &str =
    "https://github.com/matella/HeroesOfTheStormPatchNotes/releases/latest/download/";

/// Largest data file accepted from the network (patches.json is ~19 MB in schema v1).
const MAX_FILE: u64 = 256 * 1024 * 1024;

/// Resource budget for one `dataset.tar.gz` (spec 04-serveur, "Reference data"). The defaults are
/// several times the pinned release (`desktop/referential-pin.txt`, `data-d6776d73…`): 100,024,455
/// compressed bytes, 158,981,122 bytes once decompressed (1,346 files), largest image 5,393,600 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Bytes downloaded for the archive itself.
    pub archive_bytes: u64,
    /// Bytes coming out of the gzip decoder (tar headers and every entry, kept or not).
    pub expanded_bytes: u64,
    /// Bytes of one image entry (data files keep the `MAX_FILE` cap).
    pub image_bytes: u64,
}

impl Limits {
    pub const DEFAULT: Limits = Limits {
        archive_bytes: 512 * 1024 * 1024,
        expanded_bytes: 1024 * 1024 * 1024,
        image_bytes: 32 * 1024 * 1024,
    };
}

/// Where a release's files come from (an HTTP download base in production, memory in tests).
pub trait Source: Send + Sync {
    /// The bytes of release asset `name` (e.g. `manifest.json`, `enus.json`).
    fn get(&self, name: &str) -> Result<Vec<u8>, RefdataError>;
    /// Streams release asset `name` into `file`.
    fn download(&self, name: &str, file: &Path) -> Result<(), RefdataError>;
    /// Budget applied to this source's `dataset.tar.gz` (tests inject small ones).
    fn limits(&self) -> Limits {
        Limits::DEFAULT
    }
}

/// A release download base (`…/releases/latest/download/` or `…/releases/download/<tag>/`).
pub struct Http {
    base: String,
    agent: ureq::Agent,
    limits: Limits,
}

impl Http {
    pub fn new(base: &str) -> Http {
        Http {
            base: format!("{}/", base.trim_end_matches('/')),
            agent: ureq::Agent::new_with_defaults(),
            limits: Limits::DEFAULT,
        }
    }

    pub fn standalone() -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(300)))
            .build();
        Self {
            base: DEFAULT_URL.into(),
            agent: ureq::Agent::new_with_config(config),
            limits: Limits::DEFAULT,
        }
    }

    #[cfg(test)]
    pub(super) fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }
}

impl Source for Http {
    fn get(&self, name: &str) -> Result<Vec<u8>, RefdataError> {
        let url = format!("{}{name}", self.base);
        self.agent
            .get(&url)
            .call()
            .map_err(|e| RefdataError::Io(format!("{url}: {e}")))?
            .body_mut()
            .with_config()
            .limit(MAX_FILE)
            .read_to_vec()
            .map_err(|e| RefdataError::Io(format!("{url}: {e}")))
    }

    fn download(&self, name: &str, file: &Path) -> Result<(), RefdataError> {
        let url = format!("{}{name}", self.base);
        let res = self
            .agent
            .get(&url)
            .call()
            .map_err(|e| RefdataError::Io(format!("{url}: {e}")))?;
        let limit = self.limits.archive_bytes;
        // One byte past the budget is enough to refuse; the transfer stops there.
        let mut reader = res.into_body().into_reader().take(limit.saturating_add(1));
        let mut out = std::fs::File::create(file).map_err(|e| io_err(file, e))?;
        let n = std::io::copy(&mut reader, &mut out)
            .map_err(|e| RefdataError::Io(format!("{url}: {e}")))?;
        if n > limit {
            return Err(RefdataError::Budget { what: url, limit });
        }
        Ok(())
    }

    fn limits(&self) -> Limits {
        self.limits
    }
}

pub(super) fn io_err(path: &Path, e: impl std::fmt::Display) -> RefdataError {
    RefdataError::Io(format!("{}: {e}", path.display()))
}

/// Release asset name of a dataset path (`locales/enus.json` is published as `enus.json`).
pub(super) fn asset_name(path: &str) -> &str {
    DATA_FILES
        .iter()
        .find(|(p, _)| *p == path)
        .map_or(path, |(_, a)| a)
}

/// First step of a box round (blocking): the manifest, then — unless `datasetVersion` equals `last` —
/// the data files, each checked against the manifest. `Ok(None)` = unchanged, nothing else downloaded.
pub fn prepare(src: &dyn Source, last: Option<&str>) -> Result<Option<Dataset>, RefdataError> {
    let manifest = parse_manifest(&src.get("manifest.json")?)?;
    if last == Some(manifest.dataset_version.as_str()) {
        return Ok(None);
    }
    Dataset::from_files(manifest, |path| src.get(asset_name(path))).map(Some)
}

/// One box round: returns the patches that are new since the previous ingest (for `patch.new`), empty on
/// the first seed, when nothing changed, or on any failure (logged; the previous data stays).
pub async fn refresh(
    db: &PgPool,
    images_dir: &Path,
    src: Arc<dyn Source>,
    patch_image_max_file_bytes: u64,
    patch_image_cache_max_bytes: u64,
) -> Vec<(String, String)> {
    let last = last_version(db).await;
    let prepared = tokio::task::spawn_blocking({
        let src = src.clone();
        move || -> Result<Option<Rows>, RefdataError> {
            match prepare(src.as_ref(), last.as_deref())? {
                Some(ds) => build(&ds).map(Some),
                None => Ok(None),
            }
        }
    })
    .await;
    let rows = match prepared {
        Ok(Ok(Some(rows))) => rows,
        Ok(Ok(None)) => {
            tracing::info!("reference data: dataset unchanged, nothing to ingest");
            return Vec::new();
        }
        Ok(Err(e)) => {
            tracing::warn!("reference data: release refused ({e}) — previous data kept");
            return Vec::new();
        }
        Err(e) => {
            tracing::warn!("reference data: {e}");
            return Vec::new();
        }
    };
    let new = match store(db, &rows).await {
        Ok(new) => new,
        Err(e) => {
            tracing::warn!("reference data: write failed ({e}) — previous data kept");
            return Vec::new();
        }
    };
    let missing: Vec<ImageCopy> = rows
        .images
        .iter()
        .filter(|i| !images_dir.join(&i.dest).exists())
        .cloned()
        .collect();
    if !missing.is_empty() {
        let dir = images_dir.to_path_buf();
        let got =
            tokio::task::spawn_blocking(move || fetch_images(src.as_ref(), &missing, &dir)).await;
        match got {
            Ok(Ok(n)) => tracing::info!("reference data: {n} image(s) copied"),
            Ok(Err(e)) => tracing::warn!("reference data: images not copied ({e})"),
            Err(e) => tracing::warn!("reference data: images not copied ({e})"),
        }
    }
    if !rows.patch_images.is_empty() {
        let dir = images_dir.to_path_buf();
        let refs = rows.patch_images.clone();
        let report = tokio::task::spawn_blocking(move || {
            let fetcher = super::patchimg::Ureq;
            super::patchimg::fetch_missing(&dir, &fetcher, &refs, patch_image_max_file_bytes)
        })
        .await;
        match report {
            Ok(r) => {
                tracing::info!(
                    "reference data: {} patch image(s) fetched, {} already cached, {} failed",
                    r.fetched,
                    r.already_present,
                    r.failed
                );
                crate::raw::enforce_lru(
                    &images_dir.join("patch-images"),
                    patch_image_cache_max_bytes,
                )
                .await;
            }
            Err(e) => tracing::warn!("reference data: patch images not fetched ({e})"),
        }
    }
    new
}

/// Downloads `dataset.tar.gz` (checked against `dataset.tar.gz.sha256`) and copies `images` out of it.
fn fetch_images(src: &dyn Source, images: &[ImageCopy], dir: &Path) -> Result<u32, RefdataError> {
    let file = std::env::temp_dir().join(format!(
        "storm-codex-dataset-{}.tar.gz",
        uuid::Uuid::new_v4()
    ));
    fetch_images_via(src, images, dir, &file)
}

pub(super) fn fetch_images_via(
    src: &dyn Source,
    images: &[ImageCopy],
    dir: &Path,
    file: &Path,
) -> Result<u32, RefdataError> {
    let result = (|| {
        let limits = src.limits();
        src.download("dataset.tar.gz", file)?;
        // Any source is held to the compressed budget, not only the HTTP one that streams it.
        let size = std::fs::metadata(file).map_err(|e| io_err(file, e))?.len();
        if size > limits.archive_bytes {
            return Err(RefdataError::Budget {
                what: "dataset.tar.gz".into(),
                limit: limits.archive_bytes,
            });
        }
        let expected = parse_sha256_file(&src.get("dataset.tar.gz.sha256")?)?;
        check_file_hash(file, &expected)?;
        let gz = std::fs::File::open(file).map_err(|e| io_err(file, e))?;
        copy_images(gz, images, dir, &limits)
    })();
    let _ = std::fs::remove_file(file);
    result
}

/// `<hex>  <name>` (sha256sum format) → the hex digest.
pub fn parse_sha256_file(bytes: &[u8]) -> Result<String, RefdataError> {
    let text = String::from_utf8_lossy(bytes);
    let hex = text
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(hex)
    } else {
        Err(RefdataError::Manifest(
            "dataset.tar.gz.sha256: not a SHA-256".into(),
        ))
    }
}

/// Streams `reader` (the contents of `file`) through SHA-256; returns the hex digest and byte count.
fn stream_sha256(file: &Path, mut reader: impl Read) -> Result<(String, u64), RefdataError> {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut total = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| io_err(file, e))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        total += n as u64;
    }
    Ok((hex::encode(h.finalize()), total))
}

/// SHA-256 of `file`, streamed. A file over `limit` bytes is refused (checked against its size
/// first, and while reading in case it grows).
pub(super) fn file_sha256(file: &Path, limit: u64) -> Result<String, RefdataError> {
    let f = std::fs::File::open(file).map_err(|e| io_err(file, e))?;
    let too_big = || RefdataError::Budget {
        what: file.display().to_string(),
        limit,
    };
    if f.metadata().map_err(|e| io_err(file, e))?.len() > limit {
        return Err(too_big());
    }
    let (hash, n) = stream_sha256(file, f.take(limit.saturating_add(1)))?;
    if n > limit {
        return Err(too_big());
    }
    Ok(hash)
}

/// Streams `file` through SHA-256 and compares with `expected`.
pub fn check_file_hash(file: &Path, expected: &str) -> Result<(), RefdataError> {
    let f = std::fs::File::open(file).map_err(|e| io_err(file, e))?;
    let (actual, _) = stream_sha256(file, f)?;
    if actual != expected {
        return Err(RefdataError::Hash {
            path: file.display().to_string(),
            expected: expected.into(),
            actual,
        });
    }
    Ok(())
}

/// Fails the read once more than `left` bytes have come out of the gzip decoder. `over` records it,
/// because the tar reader wraps the read error.
struct Expansion<R> {
    inner: R,
    left: u64,
    over: Rc<Cell<bool>>,
}

impl<R: Read> Read for Expansion<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        match self.left.checked_sub(n as u64) {
            Some(left) => {
                self.left = left;
                Ok(n)
            }
            None => {
                self.over.set(true);
                Err(std::io::Error::other("expansion budget exceeded"))
            }
        }
    }
}

/// Visits the regular files of a gzipped tarball within `expanded_bytes` of decompressed output.
/// `cap(path)` is the largest size accepted for an entry to read (`None` = skip it unread); an
/// entry over its cap is refused before it is buffered.
fn each_entry(
    gz: impl Read,
    expanded_bytes: u64,
    cap: impl Fn(&str) -> Option<u64>,
    mut visit: impl FnMut(String, Vec<u8>) -> Result<(), RefdataError>,
) -> Result<(), RefdataError> {
    let over = Rc::new(Cell::new(false));
    let mut archive = tar::Archive::new(Expansion {
        inner: flate2::read::GzDecoder::new(gz),
        left: expanded_bytes,
        over: over.clone(),
    });
    let fail = |e: std::io::Error, path: &str| {
        if over.get() {
            RefdataError::Budget {
                what: "expanded dataset archive".into(),
                limit: expanded_bytes,
            }
        } else if path.is_empty() {
            RefdataError::Io(e.to_string())
        } else {
            RefdataError::Io(format!("{path}: {e}"))
        }
    };
    for entry in archive.entries().map_err(|e| fail(e, ""))? {
        let mut entry = entry.map_err(|e| fail(e, ""))?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry
            .path()
            .map_err(|e| fail(e, ""))?
            .to_string_lossy()
            .trim_start_matches("./")
            .to_string();
        let Some(limit) = cap(&path) else {
            continue;
        };
        if entry.size() > limit {
            return Err(RefdataError::Budget { what: path, limit });
        }
        let mut bytes = Vec::new();
        (&mut entry)
            .take(limit.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|e| fail(e, &path))?;
        if bytes.len() as u64 > limit {
            return Err(RefdataError::Budget { what: path, limit });
        }
        visit(path, bytes)?;
    }
    Ok(())
}

/// Copies the `images` found in a gzipped tarball into `dir`, each checked against its `assets.json`
/// SHA-256; files already in `dir` are kept (D3: the square icons vendored by the old API stay).
/// An image the archive does not hold is an error: a generation is complete or not staged at all.
fn copy_images(
    gz: impl Read,
    images: &[ImageCopy],
    dir: &Path,
    limits: &Limits,
) -> Result<u32, RefdataError> {
    let mut wanted: HashMap<&str, Vec<&ImageCopy>> = HashMap::new();
    for i in images {
        wanted.entry(i.source.as_str()).or_default().push(i);
    }
    let mut found = HashSet::new();
    let mut copied = 0;
    each_entry(
        gz,
        limits.expanded_bytes,
        |path| wanted.contains_key(path).then_some(limits.image_bytes),
        |path, bytes| {
            let targets = wanted.get(path.as_str()).map_or(&[][..], Vec::as_slice);
            let actual = sha256_hex(&bytes);
            for img in targets {
                if actual != img.sha256 {
                    return Err(RefdataError::Hash {
                        path: path.clone(),
                        expected: img.sha256.clone(),
                        actual: actual.clone(),
                    });
                }
                let dest = dir.join(&img.dest);
                if dest.exists() {
                    continue;
                }
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| io_err(parent, e))?;
                }
                std::fs::write(&dest, &bytes).map_err(|e| io_err(&dest, e))?;
                copied += 1;
            }
            found.insert(path);
            Ok(())
        },
    )?;
    if let Some(missing) = wanted.keys().find(|p| !found.contains(**p)) {
        return Err(RefdataError::Io(format!(
            "{missing} missing from the archive"
        )));
    }
    Ok(copied)
}

/// `datasetVersion` last ingested (`None` before the first ingest).
pub async fn last_version(db: &PgPool) -> Option<String> {
    sqlx::query_scalar::<_, Option<String>>(
        "SELECT value #>> '{}' FROM app_settings WHERE key = $1",
    )
    .bind(VERSION_KEY)
    .fetch_optional(db)
    .await
    .ok()
    .flatten()
    .flatten()
}

/// Writes `rows` in one transaction (full refresh: rows the dataset no longer has are removed) and
/// records the dataset version. Returns the new patches, none on the first seed (empty `dim_patches`).
pub async fn store(db: &PgPool, rows: &Rows) -> Result<Vec<(String, String)>, sqlx::Error> {
    let mut tx = db.begin().await?;
    let new = write_rows(&mut tx, rows).await?;
    tx.commit().await?;
    Ok(new)
}

pub(super) async fn write_rows(
    tx: &mut Transaction<'_, Postgres>,
    rows: &Rows,
) -> Result<Vec<(String, String)>, sqlx::Error> {
    let was_empty: bool = sqlx::query_scalar("SELECT NOT EXISTS (SELECT 1 FROM dim_patches)")
        .fetch_one(&mut **tx)
        .await?;

    sqlx::query("DELETE FROM dim_heroes")
        .execute(&mut **tx)
        .await?;
    for h in &rows.heroes {
        sqlx::query(
            "INSERT INTO dim_heroes (id, name, role, universe, data) VALUES ($1,$1,$2,$3,$4)
             ON CONFLICT (id) DO UPDATE SET role=EXCLUDED.role, universe=EXCLUDED.universe,
                data=EXCLUDED.data",
        )
        .bind(&h.id)
        .bind(&h.role)
        .bind(&h.universe)
        .bind(&h.data)
        .execute(&mut **tx)
        .await?;
    }

    sqlx::query("DELETE FROM dim_talents")
        .execute(&mut **tx)
        .await?;
    for t in &rows.talents {
        sqlx::query(
            "INSERT INTO dim_talents (hero_id, tier, name, tree_id, data) VALUES ($1,$2,$3,$4,$5)
             ON CONFLICT (hero_id, tier, name) DO NOTHING",
        )
        .bind(&t.hero_id)
        .bind(t.tier)
        .bind(&t.name)
        .bind(&t.tree_id)
        .bind(&t.data)
        .execute(&mut **tx)
        .await?;
    }

    let mut new = Vec::new();
    let mut ids = Vec::with_capacity(rows.patches.len());
    for p in &rows.patches {
        let inserted: bool = sqlx::query_scalar(
            "INSERT INTO dim_patches
               (internal_id, name, type, live_date, hero_count, map_count, aliases, data)
             VALUES ($1,$2,$3,$4::date::timestamptz,$5,$6,$7,$8)
             ON CONFLICT (internal_id) DO UPDATE SET name=EXCLUDED.name, type=EXCLUDED.type,
                live_date=EXCLUDED.live_date, hero_count=EXCLUDED.hero_count,
                map_count=EXCLUDED.map_count, aliases=EXCLUDED.aliases, data=EXCLUDED.data
             RETURNING (xmax = 0)",
        )
        .bind(&p.internal_id)
        .bind(&p.name)
        .bind(&p.patch_type)
        .bind(&p.live_date)
        .bind(p.hero_count)
        .bind(p.map_count)
        .bind(&p.aliases)
        .bind(&p.data)
        .fetch_one(&mut **tx)
        .await?;
        if inserted {
            new.push((p.internal_id.clone(), p.name.clone()));
        }
        ids.push(p.internal_id.clone());
    }
    sqlx::query("DELETE FROM dim_patches WHERE NOT (internal_id = ANY($1))")
        .bind(&ids)
        .execute(&mut **tx)
        .await?;

    sqlx::query("DELETE FROM patch_hero_sections")
        .execute(&mut **tx)
        .await?;
    for s in &rows.sections {
        sqlx::query(
            "INSERT INTO patch_hero_sections
               (patch_internal_id, anchor, hero_short_name, hero_name, hero_key, classification,
                short_summary, content, patch_name, patch_type, live_date, channel)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11::date::timestamptz,$12)
             ON CONFLICT (patch_internal_id, anchor) DO NOTHING",
        )
        .bind(&s.patch_internal_id)
        .bind(&s.anchor)
        .bind(&s.hero_short_name)
        .bind(&s.hero_name)
        .bind(&s.hero_key)
        .bind(&s.classification)
        .bind(&s.short_summary)
        .bind(&s.content)
        .bind(&s.patch_name)
        .bind(&s.patch_type)
        .bind(&s.live_date)
        .bind(&s.channel)
        .execute(&mut **tx)
        .await?;
    }

    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES ($1, to_jsonb($2::text))
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
    )
    .bind(VERSION_KEY)
    .bind(&rows.dataset_version)
    .execute(&mut **tx)
    .await?;
    tracing::info!(
        "reference data: dataset {} ingested — {} heroes, {} talents, {} patches ({} new), {} hero sections",
        rows.dataset_version,
        rows.heroes.len(),
        rows.talents.len(),
        rows.patches.len(),
        new.len(),
        rows.sections.len()
    );
    Ok(if was_empty { Vec::new() } else { new })
}

/// Reads every entry of a gzipped tarball into memory (`path → bytes`), keeping only `keep(path)`,
/// within `limits` (data files are capped at `MAX_FILE`, other entries at `image_bytes`).
///
/// Memory bound: everything kept is buffered at once, so peak memory can reach
/// `limits.expanded_bytes` (1 GiB by default), not just one entry. Callers pass narrow `keep`
/// filters (the data files, or only the listed images) on trusted local inputs: the desktop bundle
/// or the `refdata trim` CLI input checked against its `.sha256`.
fn read_tar(
    gz: impl Read,
    keep: impl Fn(&str) -> bool,
    limits: &Limits,
) -> Result<HashMap<String, Vec<u8>>, RefdataError> {
    let mut out = HashMap::new();
    each_entry(
        gz,
        limits.expanded_bytes,
        |path| {
            keep(path).then_some(if is_data_file(path) {
                MAX_FILE
            } else {
                limits.image_bytes
            })
        },
        |path, bytes| {
            out.insert(path, bytes);
            Ok(())
        },
    )?;
    Ok(out)
}

fn is_data_file(path: &str) -> bool {
    path == "manifest.json" || DATA_FILES.iter().any(|(p, _)| *p == path)
}

/// Parses the dataset held in `files` (`path → bytes`, as in the tarball), checked against its manifest.
fn dataset_from(files: &HashMap<String, Vec<u8>>) -> Result<Dataset, RefdataError> {
    let manifest = parse_manifest(
        files
            .get("manifest.json")
            .ok_or_else(|| RefdataError::Manifest("manifest.json missing".into()))?,
    )?;
    Dataset::from_files(manifest, |path| {
        files
            .get(path)
            .cloned()
            .ok_or_else(|| RefdataError::Io(format!("{path} missing from the archive")))
    })
}

/// Stage a legacy trimmed snapshot with the same adapter and image verifier as the feed.
pub(super) fn stage_bundle(file: &Path, dir: &Path) -> Result<(Rows, Manifest), RefdataError> {
    let open = || std::fs::File::open(file).map_err(|e| io_err(file, e));
    let files = read_tar(open()?, is_data_file, &Limits::DEFAULT)?;
    let ds = dataset_from(&files)?;
    let rows = build(&ds)?;
    copy_images(open()?, &rows.images, dir, &Limits::DEFAULT)?;
    Ok((rows, ds.manifest))
}

/// What [`trim`] produced.
#[derive(Debug)]
pub struct Trimmed {
    pub dataset_version: String,
    pub files: usize,
    pub bytes: u64,
    pub sha256: String,
}

/// `storm-codex-server refdata trim --from dataset.tar.gz --out referential.tar.gz`: checks the
/// dataset against `<from>.sha256`, then writes a bundle with only what the app reads — the manifest,
/// the four data files and the hero/map images [`build`] lists (no talent icons, no other locale).
pub fn trim(from: &Path, out: &Path) -> Result<Trimmed, RefdataError> {
    let sha_file = {
        let mut s = from.as_os_str().to_owned();
        s.push(".sha256");
        std::path::PathBuf::from(s)
    };
    let expected = parse_sha256_file(&std::fs::read(&sha_file).map_err(|e| io_err(&sha_file, e))?)?;
    check_file_hash(from, &expected)?;

    let open = || std::fs::File::open(from).map_err(|e| io_err(from, e));
    let data = read_tar(open()?, is_data_file, &Limits::DEFAULT)?;
    let ds = dataset_from(&data)?;
    let rows = build(&ds)?;
    let images: BTreeSet<&str> = rows.images.iter().map(|i| i.source.as_str()).collect();
    // Holds every listed image in memory until the bundle is written (bounded by
    // `expanded_bytes`, see `read_tar`); fine for a build-time tool on the pinned release.
    let mut kept = read_tar(open()?, |p| images.contains(p), &Limits::DEFAULT)?;
    for (path, bytes) in &kept {
        ds.manifest.verify(path, bytes)?;
    }
    kept.extend(data);

    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| io_err(parent, e))?;
    }
    let file = std::fs::File::create(out).map_err(|e| io_err(out, e))?;
    let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(
        file,
        flate2::Compression::best(),
    ));
    let mut paths: Vec<&String> = kept.keys().collect();
    paths.sort();
    for path in &paths {
        let bytes = &kept[*path];
        let mut h = tar::Header::new_gnu();
        h.set_size(bytes.len() as u64);
        h.set_mode(0o644);
        h.set_mtime(0);
        h.set_cksum();
        tar.append_data(&mut h, path, bytes.as_slice())
            .map_err(|e| io_err(out, e))?;
    }
    tar.into_inner()
        .and_then(|gz| gz.finish())
        .map_err(|e| io_err(out, e))?;
    let bytes = std::fs::metadata(out).map_err(|e| io_err(out, e))?.len();
    let sha256 = sha256_hex(&std::fs::read(out).map_err(|e| io_err(out, e))?);
    Ok(Trimmed {
        dataset_version: ds.manifest.dataset_version,
        files: paths.len(),
        bytes,
        sha256,
    })
}

/// `refdata …` subcommand. Returns the process exit code.
pub fn cli(args: &[String]) -> i32 {
    let usage =
        "usage: storm-codex-server refdata trim --from <dataset.tar.gz> --out <referential.tar.gz>";
    let flag = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .map(std::path::PathBuf::from)
    };
    match (
        args.first().map(String::as_str),
        flag("--from"),
        flag("--out"),
    ) {
        (Some("trim"), Some(from), Some(out)) => match trim(&from, &out) {
            Ok(t) => {
                println!(
                    "{}: dataset {}, {} files, {} bytes, sha256 {}",
                    out.display(),
                    t.dataset_version,
                    t.files,
                    t.bytes,
                    t.sha256
                );
                0
            }
            Err(e) => {
                eprintln!("refdata trim: {e}");
                1
            }
        },
        _ => {
            eprintln!("{usage}");
            2
        }
    }
}
