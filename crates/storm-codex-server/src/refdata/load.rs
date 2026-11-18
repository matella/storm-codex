//! The two thin loaders of the reference dataset, and the one writer they share.
//!
//! - **Box** ([`refresh`]): reads `manifest.json` from `REFDATA_URL` (a release download base), refuses a
//!   schema other than v1, skips the round when `datasetVersion` is the one last ingested, else downloads
//!   the data files, checks each against the manifest SHA-256 and writes them in one transaction. Images
//!   missing from `images_dir` come from `dataset.tar.gz` (checked against `dataset.tar.gz.sha256`).
//! - **Standalone** ([`ingest_bundle`]): reads the trimmed bundle shipped with the installer (built by
//!   [`trim`], `storm-codex-server refdata trim`), with the same checks. No network.
//!
//! Any refusal (schema, hash, shape) happens before the transaction: the previous data is kept.

use super::{
    build, parse_manifest, sha256_hex, Dataset, ImageCopy, RefdataError, Rows, DATA_FILES,
};
use sqlx::PgPool;
use std::collections::{BTreeSet, HashMap};
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

/// `app_settings` key holding the `datasetVersion` last ingested.
pub const VERSION_KEY: &str = "refdata_dataset_version";

/// Default `REFDATA_URL`: the producer's latest validated release.
pub const DEFAULT_URL: &str =
    "https://github.com/matella/HeroesOfTheStormPatchNotes/releases/latest/download/";

/// Largest data file accepted from the network (patches.json is ~19 MB in schema v1).
const MAX_FILE: u64 = 256 * 1024 * 1024;

/// Where a release's files come from (an HTTP download base in production, memory in tests).
pub trait Source: Send + Sync {
    /// The bytes of release asset `name` (e.g. `manifest.json`, `enus.json`).
    fn get(&self, name: &str) -> Result<Vec<u8>, RefdataError>;
    /// Streams release asset `name` into `file`.
    fn download(&self, name: &str, file: &Path) -> Result<(), RefdataError>;
}

/// A release download base (`…/releases/latest/download/` or `…/releases/download/<tag>/`).
pub struct Http {
    base: String,
}

impl Http {
    pub fn new(base: &str) -> Http {
        Http {
            base: format!("{}/", base.trim_end_matches('/')),
        }
    }
}

impl Source for Http {
    fn get(&self, name: &str) -> Result<Vec<u8>, RefdataError> {
        let url = format!("{}{name}", self.base);
        ureq::get(&url)
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
        let res = ureq::get(&url)
            .call()
            .map_err(|e| RefdataError::Io(format!("{url}: {e}")))?;
        let mut reader = res.into_body().into_reader();
        let mut out = std::fs::File::create(file).map_err(|e| io_err(file, e))?;
        std::io::copy(&mut reader, &mut out)
            .map_err(|e| RefdataError::Io(format!("{url}: {e}")))?;
        Ok(())
    }
}

fn io_err(path: &Path, e: impl std::fmt::Display) -> RefdataError {
    RefdataError::Io(format!("{}: {e}", path.display()))
}

/// Release asset name of a dataset path (`locales/enus.json` is published as `enus.json`).
fn asset_name(path: &str) -> &str {
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
    new
}

/// Downloads `dataset.tar.gz` (checked against `dataset.tar.gz.sha256`) and copies `images` out of it.
fn fetch_images(src: &dyn Source, images: &[ImageCopy], dir: &Path) -> Result<u32, RefdataError> {
    let file = std::env::temp_dir().join(format!(
        "storm-codex-dataset-{}.tar.gz",
        uuid::Uuid::new_v4()
    ));
    let result = (|| {
        src.download("dataset.tar.gz", &file)?;
        let expected = parse_sha256_file(&src.get("dataset.tar.gz.sha256")?)?;
        check_file_hash(&file, &expected)?;
        let gz = std::fs::File::open(&file).map_err(|e| io_err(&file, e))?;
        copy_images(gz, images, dir)
    })();
    let _ = std::fs::remove_file(&file);
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

/// Streams `file` through SHA-256 and compares with `expected`.
pub fn check_file_hash(file: &Path, expected: &str) -> Result<(), RefdataError> {
    use sha2::{Digest, Sha256};
    let mut f = std::fs::File::open(file).map_err(|e| io_err(file, e))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf).map_err(|e| io_err(file, e))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    let actual = hex::encode(h.finalize());
    if actual != expected {
        return Err(RefdataError::Hash {
            path: file.display().to_string(),
            expected: expected.into(),
            actual,
        });
    }
    Ok(())
}

/// Copies the `images` found in a gzipped tarball into `dir`, each checked against its `assets.json`
/// SHA-256; files already in `dir` are kept (D3: the square icons vendored by the old API stay).
fn copy_images(gz: impl Read, images: &[ImageCopy], dir: &Path) -> Result<u32, RefdataError> {
    let mut wanted: HashMap<&str, Vec<&ImageCopy>> = HashMap::new();
    for i in images {
        wanted.entry(i.source.as_str()).or_default().push(i);
    }
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(gz));
    let mut copied = 0;
    for entry in archive
        .entries()
        .map_err(|e| RefdataError::Io(e.to_string()))?
    {
        let mut entry = entry.map_err(|e| RefdataError::Io(e.to_string()))?;
        let path = entry
            .path()
            .map_err(|e| RefdataError::Io(e.to_string()))?
            .to_string_lossy()
            .trim_start_matches("./")
            .to_string();
        let Some(targets) = wanted.get(path.as_str()) else {
            continue;
        };
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|e| RefdataError::Io(format!("{path}: {e}")))?;
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
    let was_empty: bool = sqlx::query_scalar("SELECT NOT EXISTS (SELECT 1 FROM dim_patches)")
        .fetch_one(&mut *tx)
        .await?;

    sqlx::query("DELETE FROM dim_heroes")
        .execute(&mut *tx)
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
        .execute(&mut *tx)
        .await?;
    }

    sqlx::query("DELETE FROM dim_talents")
        .execute(&mut *tx)
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
        .execute(&mut *tx)
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
        .fetch_one(&mut *tx)
        .await?;
        if inserted {
            new.push((p.internal_id.clone(), p.name.clone()));
        }
        ids.push(p.internal_id.clone());
    }
    sqlx::query("DELETE FROM dim_patches WHERE NOT (internal_id = ANY($1))")
        .bind(&ids)
        .execute(&mut *tx)
        .await?;

    sqlx::query("DELETE FROM patch_hero_sections")
        .execute(&mut *tx)
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
        .execute(&mut *tx)
        .await?;
    }

    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES ($1, to_jsonb($2::text))
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
    )
    .bind(VERSION_KEY)
    .bind(&rows.dataset_version)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
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

/// Reads every entry of a gzipped tarball into memory (`path → bytes`), keeping only `keep(path)`.
fn read_tar(
    gz: impl Read,
    keep: impl Fn(&str) -> bool,
) -> Result<HashMap<String, Vec<u8>>, RefdataError> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(gz));
    let mut out = HashMap::new();
    for entry in archive
        .entries()
        .map_err(|e| RefdataError::Io(e.to_string()))?
    {
        let mut entry = entry.map_err(|e| RefdataError::Io(e.to_string()))?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry
            .path()
            .map_err(|e| RefdataError::Io(e.to_string()))?
            .to_string_lossy()
            .trim_start_matches("./")
            .to_string();
        if !keep(&path) {
            continue;
        }
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|e| RefdataError::Io(format!("{path}: {e}")))?;
        out.insert(path, bytes);
    }
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

/// Standalone: ingests the bundled trimmed snapshot (`REFERENTIAL_FILE`). Same checks as the box; the
/// images come from the bundle. Returns the new patches (none on the first seed).
pub async fn ingest_bundle(
    db: &PgPool,
    images_dir: &Path,
    file: &Path,
) -> Result<Vec<(String, String)>, String> {
    let file = file.to_path_buf();
    let dir = images_dir.to_path_buf();
    let prepared = tokio::task::spawn_blocking(move || -> Result<Rows, RefdataError> {
        let bytes = std::fs::read(&file).map_err(|e| io_err(&file, e))?;
        let files = read_tar(bytes.as_slice(), |_| true)?;
        let ds = dataset_from(&files)?;
        let rows = build(&ds)?;
        // Images: the bundle holds exactly the ones `build` lists; check each, keep existing files.
        let mut copied = 0;
        for img in &rows.images {
            let Some(data) = files.get(&img.source) else {
                continue; // a map without art: the front falls back
            };
            ds.manifest.verify(&img.source, data)?;
            let dest = dir.join(&img.dest);
            if dest.exists() {
                continue;
            }
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| io_err(parent, e))?;
            }
            std::fs::write(&dest, data).map_err(|e| io_err(&dest, e))?;
            copied += 1;
        }
        tracing::info!("reference data (bundle): {copied} image(s) copied");
        Ok(rows)
    })
    .await;
    let rows = match prepared {
        Ok(Ok(rows)) => rows,
        Ok(Err(e)) => {
            tracing::warn!("reference data: bundled snapshot refused ({e})");
            return Err(e.to_string());
        }
        Err(e) => return Err(e.to_string()),
    };
    store(db, &rows).await.map_err(|e| {
        tracing::warn!("reference data: write failed ({e})");
        e.to_string()
    })
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
    let data = read_tar(open()?, is_data_file)?;
    let ds = dataset_from(&data)?;
    let rows = build(&ds)?;
    let images: BTreeSet<&str> = rows.images.iter().map(|i| i.source.as_str()).collect();
    let mut kept = read_tar(open()?, |p| images.contains(p))?;
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
