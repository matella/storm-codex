//! Tests of the reference dataset adapter (spec 2026-11-15, acceptance criteria 1–8).
//!
//! - The committed fixture `tests/fixtures/refdata/dataset/` is cut from release
//!   `data-c8d8ff8f5357d9e8b644d416376bb541c3dd31ec3406f60c9f41f44037483bab` (9 heroes, 3 maps, 5 patches,
//!   placeholder image bytes; the manifest lists the hashes of the cut files).
//! - `REFDATA_DATASET_DIR` = a folder holding that whole release's `manifest.json`, `game.json`,
//!   `enus.json`, `patches.json`, `assets.json` (CI downloads them): full counts and the corpus diff.
//! - `DATABASE_URL`: ingest, alias and endpoint-shape tests, each on its own throwaway database
//!   (a full refresh of `dim_*` must not race the other tests' reference data).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::load::{self, Source};
use super::*;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/refdata/dataset")
}

/// Release-like source over a folder laid out as the tarball (`locales/enus.json`), recording requests.
struct DirSource {
    dir: PathBuf,
    asked: Mutex<Vec<String>>,
    tamper: Option<&'static str>,
}

impl DirSource {
    fn new(dir: PathBuf) -> DirSource {
        DirSource {
            dir,
            asked: Mutex::new(Vec::new()),
            tamper: None,
        }
    }
    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

impl Source for DirSource {
    fn get(&self, name: &str) -> Result<Vec<u8>, RefdataError> {
        self.asked.lock().unwrap().push(name.into());
        let rel = if name == "enus.json" {
            "locales/enus.json"
        } else {
            name
        };
        let mut bytes =
            std::fs::read(self.dir.join(rel)).map_err(|e| RefdataError::Io(e.to_string()))?;
        if self.tamper == Some(name) {
            bytes.extend_from_slice(b" ");
        }
        Ok(bytes)
    }
    fn download(&self, name: &str, file: &Path) -> Result<(), RefdataError> {
        self.asked.lock().unwrap().push(name.into());
        std::fs::copy(self.dir.join(name), file).map_err(|e| RefdataError::Io(e.to_string()))?;
        Ok(())
    }
}

fn fixture() -> Dataset {
    let src = DirSource::new(fixture_dir());
    load::prepare(&src, None).unwrap().unwrap()
}

/// Reads a whole release laid out as published (`enus.json` at the top level).
fn full_release() -> Option<Dataset> {
    let dir = PathBuf::from(std::env::var_os("REFDATA_DATASET_DIR")?);
    let get =
        |name: &str| std::fs::read(dir.join(name)).map_err(|e| RefdataError::Io(e.to_string()));
    let manifest = parse_manifest(&get("manifest.json").unwrap()).unwrap();
    Some(
        Dataset::from_files(manifest, |path| {
            get(path.strip_prefix("locales/").unwrap_or(path))
        })
        .unwrap(),
    )
}

const UNIVERSES: [&str; 5] = ["Warcraft", "StarCraft", "Diablo", "Overwatch", "Nexus"];

#[test]
fn fixture_heroes_use_the_parser_name_the_front_universes_and_the_old_icon_slugs() {
    let rows = build(&fixture()).unwrap();
    assert_eq!(rows.heroes.len(), 9);
    for h in &rows.heroes {
        assert!(UNIVERSES.contains(&h.universe.as_deref().unwrap()), "{h:?}");
        assert!(h.role.is_some(), "{}", h.id);
    }
    let hero = |id: &str| rows.heroes.iter().find(|h| h.id == id).unwrap();
    // attributeId → parser name, not the dataset key (Amazon) nor the English name (Lúcio).
    assert_eq!(hero("Cassia").data["key"], "Amazon");
    assert_eq!(hero("Lucio").data["icon"], "/images/heroes/lucio.png");
    assert_eq!(
        hero("The Lost Vikings").data["icon"],
        "/images/heroes/thelostvikings.png"
    );
    assert_eq!(hero("Abathur").universe.as_deref(), Some("StarCraft"));
    assert_eq!(hero("Abathur").role.as_deref(), Some("Specialist"));
}

#[test]
fn fixture_talents_keep_every_talent_id_with_tier_and_english_name() {
    let ds = fixture();
    let rows = build(&ds).unwrap();
    let mut ids: Vec<&str> = ds.game["heroes"]
        .as_object()
        .unwrap()
        .values()
        .flat_map(|h| h["structure"]["talents"].as_object().unwrap().values())
        .flat_map(|l| l.as_array().unwrap())
        .map(|t| t["talentId"].as_str().unwrap())
        .collect();
    ids.sort_unstable();
    ids.dedup();
    let mut got: Vec<&str> = rows.talents.iter().map(|t| t.tree_id.as_str()).collect();
    got.sort_unstable();
    got.dedup();
    assert_eq!(got, ids);
    let t = rows
        .talents
        .iter()
        .find(|t| t.tree_id == "AbathurMasteryPressurizedGlands")
        .unwrap();
    assert_eq!(
        (t.hero_id.as_str(), t.tier, t.name.as_str()),
        ("Abathur", 1, "Pressurized Glands")
    );
    assert_eq!(t.data["icon"], "storm_ui_icon_abathur_spikeburst.png");
    assert_eq!(t.data["type"], "W");
    assert!(t.data["description"]
        .as_str()
        .unwrap()
        .starts_with("Increases the range"));
}

#[test]
fn fixture_patches_keep_aliases_types_availability_and_sources() {
    let rows = build(&fixture()).unwrap();
    assert_eq!(rows.patches.len(), 5);
    let p = |id: &str| rows.patches.iter().find(|p| p.internal_id == id).unwrap();

    let old = p("live:29562");
    assert_eq!(old.aliases, ["hotfix-patch-notes-march-25-2014"]);
    assert_eq!(old.patch_type, "Hotfix Patch");
    assert_eq!(old.live_date.as_deref(), Some("2014-03-25"));
    assert_eq!(
        old.data["officialLink"],
        "https://us.battle.net/forums/en/heroes/topic/12250465022#post-1"
    );
    assert_eq!(old.data["hasContent"], true);

    let meta = p("live:30019");
    assert_eq!(meta.data["hasContent"], false);
    assert_eq!(meta.data["communityOnly"], true);
    assert_eq!(meta.data["officialLink"], J::Null);
    assert_eq!(
        meta.data["detail"]["reason"],
        "No validated article content acquired"
    );
    assert_eq!(meta.data["detail"]["content"], "");

    let ptr = p("source:bluetracker:us-en:24317084");
    assert_eq!(ptr.patch_type, "PTR");
    assert_eq!(ptr.data["channel"], "ptr");
    assert!(rows
        .sections
        .iter()
        .filter(|s| s.patch_internal_id == ptr.internal_id)
        .all(|s| s.channel == "ptr"));
}

#[test]
fn fixture_hero_sections_one_per_hero_section_of_available_content() {
    let ds = fixture();
    let rows = build(&ds).unwrap();
    let expected = ds
        .patches
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["availability"] == "available")
        .flat_map(|p| p["contents"]["enus"]["sections"].as_array().unwrap())
        .filter(|s| s["heroId"].is_string())
        .count();
    assert_eq!(rows.sections.len(), expected);
    assert!(expected > 0);
    let p = rows
        .patches
        .iter()
        .find(|p| p.internal_id == "source:bluetracker:us-en:24252009")
        .unwrap();
    assert_eq!(
        usize::try_from(p.hero_count).unwrap(),
        rows.sections
            .iter()
            .filter(|s| s.patch_internal_id == p.internal_id)
            .count()
    );
    for s in &rows.sections {
        assert_eq!(s.hero_key, hero_key(&s.hero_name));
        assert!(
            ["BUFF", "NERF", "MIXED", "REWORK", "BUGFIX", ""].contains(&s.classification.as_str())
        );
    }
}

#[test]
fn fixture_detail_is_rebuilt_in_the_old_shape_with_scrollable_anchors() {
    let rows = build(&fixture()).unwrap();
    let heading = regex::Regex::new(r"<h[1-6][\s>]").unwrap();
    for p in &rows.patches {
        let d = &p.data["detail"];
        let content = d["content"].as_str().unwrap();
        for s in d["sections"].as_array().unwrap() {
            assert!(
                ["Hero", "Battleground", "General"].contains(&s["sectionType"].as_str().unwrap())
            );
        }
        let toc = d["tableOfContents"].as_array().unwrap();
        // every heading of the content carries the anchor of its TOC entry (counted from the end)
        let headings = heading.find_iter(content).count();
        for t in toc.iter().skip(toc.len().saturating_sub(headings)) {
            let anchor = t["anchor"].as_str().unwrap();
            assert!(
                content.contains(&format!("id=\"{anchor}\"")),
                "{} {anchor}",
                p.internal_id
            );
        }
    }
    let p = rows
        .patches
        .iter()
        .find(|p| p.internal_id == "source:bluetracker:us-en:24145596")
        .unwrap();
    let hero = p.data["detail"]["sections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["sectionType"] == "Hero")
        .unwrap();
    assert!(p.data["detail"]["content"]
        .as_str()
        .unwrap()
        .contains(&format!("id=\"{}\"", hero["anchor"].as_str().unwrap())));
}

#[test]
fn fixture_images_map_portraits_and_map_art_to_the_slugs_the_front_requests() {
    let rows = build(&fixture()).unwrap();
    let dest = |d: &str| rows.images.iter().find(|i| i.dest == d);
    assert_eq!(
        dest("heroes/abathur.png").unwrap().source,
        "images/heroportraits/storm_ui_ingame_hero_loadingscreen_abathur.png"
    );
    // D3: a bundled square heroSelect icon wins over the loading portrait.
    assert!(dest("heroes/thelostvikings.png")
        .unwrap()
        .source
        .contains("heroselect"));
    assert!(dest("battlegrounds/alterac-pass.png").is_some());
    assert!(dest("battlegrounds/garden-of-terror.png").is_some());
    assert!(dest("battlegrounds/silver-city.png").is_some());
    assert!(rows.images.iter().all(|i| i.sha256.len() == 64));
}

#[test]
fn a_tampered_file_is_refused() {
    let mut src = DirSource::new(fixture_dir());
    src.tamper = Some("patches.json");
    let err = load::prepare(&src, None).err().unwrap();
    assert!(
        matches!(err, RefdataError::Hash { ref path, .. } if path == "patches.json"),
        "{err}"
    );
}

#[test]
fn a_schema_v2_manifest_is_refused_before_any_data_file() {
    let mut m: J =
        serde_json::from_slice(&std::fs::read(fixture_dir().join("manifest.json")).unwrap())
            .unwrap();
    m["schemaVersion"] = J::from(2);
    let err = parse_manifest(&serde_json::to_vec(&m).unwrap())
        .err()
        .unwrap();
    assert!(matches!(err, RefdataError::Schema(2)), "{err}");
}

#[test]
fn an_unchanged_dataset_version_downloads_only_the_manifest() {
    let src = DirSource::new(fixture_dir());
    let version = parse_manifest(&std::fs::read(fixture_dir().join("manifest.json")).unwrap())
        .unwrap()
        .dataset_version;
    assert!(load::prepare(&src, Some(&version)).unwrap().is_none());
    assert_eq!(src.asked(), ["manifest.json"]);

    let src = DirSource::new(fixture_dir());
    assert!(load::prepare(&src, Some("older")).unwrap().is_some());
    assert_eq!(
        src.asked(),
        [
            "manifest.json",
            "game.json",
            "enus.json",
            "patches.json",
            "assets.json"
        ]
    );
}

#[test]
fn headings_get_their_section_ids_counted_from_the_end() {
    let sec = |id: &str, level: i64, heading: &str| json!({"id": id, "level": level, "heading": heading, "html": "<p>x</p>"});
    let (a, b, c) = (
        sec("section-0", 1, "Intro"),
        sec("section-1", 3, "General"),
        sec("section-2", 4, "Abathur"),
    );
    // one leading section without a heading in the article
    assert_eq!(
        anchored_html("<p>intro</p><h3>General</h3><h4 class=\"x\">Abathur</h4>", &[&a, &b, &c]),
        "<p>intro</p><h3 id=\"section-1\">General</h3><h4 id=\"section-2\" class=\"x\">Abathur</h4>"
    );
    // no heading, one section: the article as published
    assert_eq!(anchored_html("<p>only</p>", &[&a]), "<p>only</p>");
    // headings that do not line up: rebuilt from the sections
    let rebuilt = anchored_html("<h2>Other</h2>", &[&b, &c]);
    assert!(
        rebuilt.starts_with("<h3 id=\"section-1\">General</h3>"),
        "{rebuilt}"
    );
    assert!(rebuilt.contains("<h4 id=\"section-2\">Abathur</h4>"));
}

#[test]
fn slugs_and_keys() {
    assert_eq!(hero_slug("Lt. Morales"), "ltmorales");
    assert_eq!(hero_slug("Lúcio"), "lucio");
    assert_eq!(hero_slug("Kel'Thuzad"), "kelthuzad");
    assert_eq!(hero_key("The Lost Vikings"), "lostvikings");
    assert_eq!(map_slug("Blackheart's Bay"), "blackhearts-bay");
    assert_eq!(map_slug("Checkpoint: Hanamura"), "checkpoint:-hanamura");
    assert_eq!(universe("Starcraft"), "StarCraft");
    assert_eq!(universe("Classic"), "Nexus");
}

/// Criterion 1 on the whole release (`REFDATA_DATASET_DIR`).
#[test]
fn full_release_counts() {
    let Some(ds) = full_release() else {
        eprintln!("REFDATA_DATASET_DIR missing → full-release counts skipped");
        return;
    };
    let rows = build(&ds).unwrap();
    assert_eq!(rows.heroes.len(), 91);
    assert!(rows
        .heroes
        .iter()
        .all(|h| UNIVERSES.contains(&h.universe.as_deref().unwrap_or(""))));
    let mut ids: Vec<&str> = ds.game["heroes"]
        .as_object()
        .unwrap()
        .values()
        .flat_map(|h| h["structure"]["talents"].as_object().unwrap().values())
        .flat_map(|l| l.as_array().unwrap())
        .map(|t| t["talentId"].as_str().unwrap())
        .collect();
    ids.sort_unstable();
    ids.dedup();
    let mut got: Vec<&str> = rows.talents.iter().map(|t| t.tree_id.as_str()).collect();
    got.sort_unstable();
    got.dedup();
    assert_eq!(got, ids);
    assert_eq!(rows.patches.len(), 276);
    let hero_sections = ds
        .patches
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["availability"] == "available")
        .flat_map(|p| p["contents"]["enus"]["sections"].as_array().unwrap())
        .filter(|s| s["heroId"].is_string())
        .count();
    assert_eq!(rows.sections.len(), hero_sections);
    // every hero portrait and every map art of the release is listed
    assert_eq!(
        rows.images
            .iter()
            .filter(|i| i.dest.starts_with("heroes/"))
            .count(),
        91
    );
    assert_eq!(
        rows.images
            .iter()
            .filter(|i| i.dest.starts_with("battlegrounds/"))
            .count(),
        35
    );
    let alias = rows
        .patches
        .iter()
        .find(|p| {
            p.aliases
                .iter()
                .any(|a| a == "hotfix-patch-notes-march-25-2014")
        })
        .unwrap();
    assert_eq!(alias.internal_id, "live:29562");
    eprintln!(
        "full release: {} heroes, {} talents, {} patches, {} hero sections, {} images",
        rows.heroes.len(),
        rows.talents.len(),
        rows.patches.len(),
        rows.sections.len(),
        rows.images.len()
    );
}

/// Criterion 2: the played-hero correction gives the same result with the dataset talents as with the
/// old API's (`tests/fixtures/refdata/old-talent-lut.json`, captured from the old service before it was
/// retired), on every player of the committed mini-corpus.
#[test]
fn talent_hero_corrections_match_the_old_reference_data_on_the_mini_corpus() {
    let Some(ds) = full_release() else {
        eprintln!("REFDATA_DATASET_DIR missing → corpus talent-hero diff skipped");
        return;
    };
    let rows = build(&ds).unwrap();
    let new_lut: std::collections::HashMap<String, String> = rows
        .talents
        .iter()
        .map(|t| (t.tree_id.clone(), t.hero_id.clone()))
        .collect();
    let old_lut: std::collections::HashMap<String, String> = serde_json::from_slice(
        &std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/refdata/old-talent-lut.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let replays: Vec<PathBuf> = ["storm-replay/tests/data", "storm-stats/tests/data"]
        .iter()
        .flat_map(|d| std::fs::read_dir(root.join(d)).unwrap())
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "StormReplay"))
        .collect();
    assert!(replays.len() >= 4, "{replays:?}");
    let mut players = 0;
    for path in &replays {
        let out = storm_stats::process_replay(path, &path.to_string_lossy());
        let Some(ps) = out.players.as_ref() else {
            continue;
        };
        let mut old = crate::project::hero_corrections(ps, &old_lut)
            .into_iter()
            .collect::<Vec<_>>();
        let mut new = crate::project::hero_corrections(ps, &new_lut)
            .into_iter()
            .collect::<Vec<_>>();
        old.sort();
        new.sort();
        assert_eq!(new, old, "{}", path.display());
        // every talent the old data resolved still resolves, to the same hero
        for p in ps.values() {
            for tid in p["talents"]
                .as_object()
                .into_iter()
                .flatten()
                .filter_map(|(_, v)| v.as_str())
            {
                if let Some(h) = old_lut.get(tid) {
                    assert_eq!(new_lut.get(tid), Some(h), "{tid}");
                }
            }
            players += 1;
        }
    }
    assert!(players >= 40, "{players} players");
    eprintln!(
        "talent-hero diff: {} replays, {players} players, no difference",
        replays.len()
    );
}

// ---- Postgres ----

/// A fresh database (migrated) on the `DATABASE_URL` server, dropped by [`drop_db`].
pub(crate) async fn isolated_db() -> Option<(sqlx::PgPool, String)> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let admin = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .expect("connexion");
    let name = format!("refdata_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}")))
        .execute(&admin)
        .await
        .expect("create database");
    admin.close().await;
    let opts: sqlx::postgres::PgConnectOptions = url.parse().expect("DATABASE_URL");
    let db = sqlx::postgres::PgPoolOptions::new()
        .connect_with(opts.database(&name))
        .await
        .expect("connexion");
    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .expect("migrations");
    Some((db, name))
}

pub(crate) async fn drop_db(db: sqlx::PgPool, name: &str) {
    db.close().await;
    if let Ok(url) = std::env::var("DATABASE_URL") {
        if let Ok(admin) = sqlx::PgPool::connect(&url).await {
            let _ = sqlx::query(sqlx::AssertSqlSafe(format!(
                "DROP DATABASE IF EXISTS {name} WITH (FORCE)"
            )))
            .execute(&admin)
            .await;
        }
    }
}

/// Packs the fixture as a release `dataset.tar.gz` (+ its `.sha256`) in `dir`.
pub(crate) fn fixture_tarball(dir: &Path) -> PathBuf {
    let file = dir.join("dataset.tar.gz");
    let gz = flate2::write::GzEncoder::new(
        std::fs::File::create(&file).unwrap(),
        flate2::Compression::fast(),
    );
    let mut tar = tar::Builder::new(gz);
    tar.append_dir_all(".", fixture_dir()).unwrap();
    tar.into_inner().unwrap().finish().unwrap();
    let sha = sha256_hex(&std::fs::read(&file).unwrap());
    std::fs::write(
        dir.join("dataset.tar.gz.sha256"),
        format!("{sha}  dataset.tar.gz\n"),
    )
    .unwrap();
    file
}

/// A bundle laid out like the trimmed snapshot (the fixture, whole) whose manifest says `version`:
/// two versions give two files with different hashes, both valid.
pub(crate) fn bundle_with_version(file: &Path, version: &str) {
    let gz = flate2::write::GzEncoder::new(
        std::fs::File::create(file).unwrap(),
        flate2::Compression::fast(),
    );
    let mut tar = tar::Builder::new(gz);
    let dir = tempfile::tempdir().unwrap();
    let copy = fixture_dir_copy(dir.path());
    let mut m: J =
        serde_json::from_slice(&std::fs::read(copy.join("manifest.json")).unwrap()).unwrap();
    m["datasetVersion"] = J::from(version);
    std::fs::write(copy.join("manifest.json"), serde_json::to_vec(&m).unwrap()).unwrap();
    std::fs::remove_file(copy.join("dataset.tar.gz")).unwrap();
    std::fs::remove_file(copy.join("dataset.tar.gz.sha256")).unwrap();
    tar.append_dir_all(".", &copy).unwrap();
    tar.into_inner().unwrap().finish().unwrap();
}

async fn count(db: &sqlx::PgPool, table: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
        .fetch_one(db)
        .await
        .unwrap()
}

#[test]
fn trim_keeps_only_what_the_app_reads_and_checks_the_release_hash() {
    let tmp = tempfile::tempdir().unwrap();
    let from = fixture_tarball(tmp.path());
    let out = tmp.path().join("out/referential.tar.gz");
    let t = load::trim(&from, &out).unwrap();
    // manifest + 4 data files + 12 images (9 portraits, 3 map arts); the talent icon is dropped
    assert_eq!(t.files, 17);
    let names: Vec<String> = tar::Archive::new(flate2::read::GzDecoder::new(
        std::fs::File::open(&out).unwrap(),
    ))
    .entries()
    .unwrap()
    .map(|e| e.unwrap().path().unwrap().to_string_lossy().into_owned())
    .collect();
    assert!(names.contains(&"locales/enus.json".to_string()));
    assert!(!names
        .iter()
        .any(|n| n.contains("hud_btn_bg_ability_cancel")));

    // a dataset that does not match its .sha256 is refused
    std::fs::write(
        tmp.path().join("dataset.tar.gz.sha256"),
        format!("{}  dataset.tar.gz\n", "0".repeat(64)),
    )
    .unwrap();
    assert!(matches!(
        load::trim(&from, &out),
        Err(RefdataError::Hash { .. })
    ));
}

#[tokio::test]
async fn store_fills_the_tables_and_notifies_only_patches_added_after_the_first_seed() {
    let Some((db, name)) = isolated_db().await else {
        eprintln!("DATABASE_URL missing → refdata store test skipped");
        return;
    };
    let rows = build(&fixture()).unwrap();
    assert!(
        load::store(&db, &rows).await.unwrap().is_empty(),
        "first seed: no notification"
    );
    assert_eq!(count(&db, "dim_heroes").await, 9);
    assert_eq!(count(&db, "dim_patches").await, 5);
    assert_eq!(
        count(&db, "patch_hero_sections").await,
        i64::try_from(rows.sections.len()).unwrap()
    );
    assert!(count(&db, "dim_talents").await > 0);
    assert_eq!(
        load::last_version(&db).await.as_deref(),
        Some(rows.dataset_version.as_str())
    );

    let mut next = rows.clone();
    let mut extra = next.patches[0].clone();
    extra.internal_id = "live:99999".into();
    extra.name = "Next Patch".into();
    next.patches.push(extra);
    next.patches.remove(1);
    assert_eq!(
        load::store(&db, &next).await.unwrap(),
        [("live:99999".to_string(), "Next Patch".to_string())]
    );
    assert_eq!(
        count(&db, "dim_patches").await,
        5,
        "a patch the dataset dropped is removed"
    );
    drop_db(db, &name).await;
}

#[tokio::test]
async fn a_refused_round_keeps_the_previous_data() {
    let Some((db, name)) = isolated_db().await else {
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let images = tmp.path().join("images");
    let dir = fixture_dir_copy(tmp.path());

    // a tampered release is refused: nothing written
    let mut bad = DirSource::new(dir.clone());
    bad.tamper = Some("game.json");
    assert!(load::refresh(&db, &images, Arc::new(bad)).await.is_empty());
    assert_eq!(count(&db, "dim_heroes").await, 0);

    // a good release is ingested, images come from the verified tarball
    let src = Arc::new(DirSource::new(dir.clone()));
    load::refresh(&db, &images, src.clone()).await;
    assert_eq!(count(&db, "dim_heroes").await, 9);
    assert!(images.join("heroes/abathur.png").is_file());
    assert!(images.join("battlegrounds/silver-city.png").is_file());
    assert!(src.asked().contains(&"dataset.tar.gz".to_string()));

    // the next round with the same datasetVersion asks only for the manifest
    let again = Arc::new(DirSource::new(dir.clone()));
    load::refresh(&db, &images, again.clone()).await;
    assert_eq!(again.asked(), ["manifest.json"]);

    // a schema v2 manifest is refused: the data stays
    let mut m: J =
        serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap()).unwrap();
    m["schemaVersion"] = J::from(2);
    m["datasetVersion"] = J::from("next");
    std::fs::write(dir.join("manifest.json"), serde_json::to_vec(&m).unwrap()).unwrap();
    let v2 = Arc::new(DirSource::new(dir));
    load::refresh(&db, &images, v2.clone()).await;
    assert_eq!(v2.asked(), ["manifest.json"]);
    assert_eq!(count(&db, "dim_heroes").await, 9);
    assert_eq!(count(&db, "dim_patches").await, 5);
    drop_db(db, &name).await;
}

/// Copies the fixture to `<tmp>/dataset` so a test can alter it; returns that folder.
fn fixture_dir_copy(tmp: &Path) -> PathBuf {
    fn copy(src: &Path, dst: &Path) {
        std::fs::create_dir_all(dst).unwrap();
        for e in std::fs::read_dir(src).unwrap() {
            let p = e.unwrap().path();
            let t = dst.join(p.file_name().unwrap());
            if p.is_dir() {
                copy(&p, &t);
            } else {
                std::fs::copy(&p, &t).unwrap();
            }
        }
    }
    let dst = tmp.join("dataset");
    copy(&fixture_dir(), &dst);
    // the release also publishes dataset.tar.gz(.sha256) next to the files
    let packed = tmp.join("pack");
    std::fs::create_dir_all(&packed).unwrap();
    fixture_tarball(&packed);
    std::fs::copy(packed.join("dataset.tar.gz"), dst.join("dataset.tar.gz")).unwrap();
    std::fs::copy(
        packed.join("dataset.tar.gz.sha256"),
        dst.join("dataset.tar.gz.sha256"),
    )
    .unwrap();
    dst
}

#[tokio::test]
async fn the_trimmed_bundle_seeds_offline() {
    let Some((db, name)) = isolated_db().await else {
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("referential.tar.gz");
    load::trim(&fixture_tarball(tmp.path()), &out).unwrap();
    let images = tmp.path().join("images");
    load::ingest_bundle(&db, &images, &out).await.unwrap();
    assert_eq!(count(&db, "dim_heroes").await, 9);
    assert_eq!(count(&db, "dim_patches").await, 5);
    assert!(images.join("heroes/lucio.png").is_file());
    drop_db(db, &name).await;
}

/// Key sets of a JSON value (recursively: objects by key, arrays by their first element).
fn shape(v: &J) -> J {
    match v {
        J::Object(m) => J::Object(m.iter().map(|(k, v)| (k.clone(), shape(v))).collect()),
        J::Array(a) => J::Array(a.first().map(shape).into_iter().collect()),
        _ => J::Null,
    }
}

fn keys(v: &J) -> Vec<&str> {
    let mut k: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    k.sort_unstable();
    k
}

/// Criteria 3 and 4: the public endpoints keep the old JSON shape (the keys the old API / the front
/// read; additions listed explicitly), and an old internalId resolves to its patch.
#[tokio::test]
async fn endpoints_keep_their_shape_and_old_ids_resolve() {
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::util::ServiceExt;
    let Some(state) = crate::api_tests::test_state(None).await else {
        return;
    };
    let Some((db, name)) = isolated_db().await else {
        return;
    };
    let mut state = state;
    state.db = db.clone();
    load::store(&db, &build(&fixture()).unwrap()).await.unwrap();
    let get = |uri: &'static str| {
        let app = crate::api_tests::app(&state);
        async move {
            let resp = app
                .oneshot(Request::get(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            let bytes = resp.into_body().collect().await.unwrap().to_bytes();
            serde_json::from_slice::<J>(&bytes).unwrap()
        }
    };

    let heroes = get("/api/dim/heroes").await;
    assert_eq!(keys(&heroes["Abathur"]), ["icon", "role", "universe"]);
    assert_eq!(heroes["Abathur"]["icon"], "/images/heroes/abathur.png");
    let talents = get("/api/dim/talents").await;
    assert_eq!(
        keys(&talents["AbathurMasteryPressurizedGlands"]),
        ["hero", "icon", "name", "tier"]
    );

    let list = get("/api/patches").await;
    let item = shape(&list)["items"][0].clone();
    assert_eq!(
        keys(&item),
        [
            "channel",
            "communityOnly", // added (spec 2026-11-15)
            "hasContent",
            "heroCount",
            "internalId",
            "liveDate",
            "mapCount",
            "officialLink",
            "patchName",
            "patchType",
        ]
    );

    // criterion 4: the old id answers with the canonical patch
    let d = get("/api/patches/hotfix-patch-notes-march-25-2014").await;
    assert_eq!(d["internalId"], "live:29562");
    assert_eq!(d["patchName"], "Tech Alpha Balance Update");
    let d = get("/api/patches/source:bluetracker:us-en:24252009").await;
    let s = shape(&d);
    for k in [
        "internalId",
        "patchName",
        "patchType",
        "liveDate",
        "officialLink",
        "content",
        "tableOfContents",
        "sections",
    ] {
        assert!(s.get(k).is_some(), "detail lost {k}");
    }
    assert_eq!(
        keys(&s["tableOfContents"][0]),
        ["anchor", "headingLevel", "order", "sectionType", "title"]
    );
    assert_eq!(
        keys(&s["sections"][0]),
        [
            "anchor",
            "classification",
            "content",
            "entityName",
            "headingLevel",
            "heroShortName",
            "order",
            "sectionType",
            "shortSummary"
        ]
    );
    assert_eq!(get("/api/patches/no-such-patch").await, J::Null);

    let changes = get("/api/hero-changes").await;
    assert_eq!(
        keys(&shape(&changes)[0]),
        [
            "anchor",
            "classification",
            "content",
            "heroName",
            "liveDate",
            "patchInternalId",
            "patchName",
            "patchType",
            "shortSummary"
        ]
    );
    // PTR candidates are not live changes
    assert!(changes
        .as_array()
        .unwrap()
        .iter()
        .all(|c| c["patchInternalId"] != "source:bluetracker:us-en:24317084"));
    let heroes_changed = get("/api/hero-changes/heroes").await;
    assert_eq!(
        keys(&shape(&heroes_changed)[0]),
        [
            "count",
            "heroName",
            "latestClass",
            "latestDate",
            "latestPatch"
        ]
    );
    let hero_patches = get("/api/hero/Thrall/patches").await;
    assert!(!hero_patches.as_array().unwrap().is_empty());
    drop_db(db, &name).await;
}
