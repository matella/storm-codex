//! Reference-data adapter (spec `docs/specs/2026-11-15-reference-dataset-adapter-design.md`).
//!
//! The hero/talent/map/patch reference data comes from the HotsPatchNotes **dataset** (schema v1,
//! published as GitHub releases). This module turns that dataset into the rows of `dim_heroes`,
//! `dim_talents`, `dim_patches` and `patch_hero_sections`, plus the list of images to copy. It does no
//! I/O: the two thin loaders in [`load`] (box: download a release; standalone: read the bundled trimmed
//! file) feed it bytes and write what it returns. One pipeline, one source (hard rule 7).
//!
//! Join keys kept from the old API:
//! - hero: the **parser name** (`dim_heroes.id`/`name`), via the dataset `attributeId` →
//!   `storm_stats::constants::hero_attribute()`; fallback the English name, then the dataset key;
//! - talent: `tree_id` = the dataset `talentId` (the parser's `TierNChoice` values).

pub mod classify;
pub mod load;

use serde_json::{json, Map, Value as J};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// The only dataset schema this adapter reads. Anything else is refused before a write.
pub const SCHEMA_VERSION: u64 = 1;

/// The data files the adapter reads: (path in the manifest/tarball, release asset name).
pub const DATA_FILES: [(&str, &str); 4] = [
    ("game.json", "game.json"),
    ("locales/enus.json", "enus.json"),
    ("patches.json", "patches.json"),
    ("assets.json", "assets.json"),
];

#[derive(Debug, thiserror::Error)]
pub enum RefdataError {
    #[error("manifest: {0}")]
    Manifest(String),
    #[error("dataset schema version {0} is not supported (expected {SCHEMA_VERSION})")]
    Schema(u64),
    #[error("{0}: not listed in the manifest")]
    Unlisted(String),
    #[error("{path}: SHA-256 mismatch (manifest {expected}, file {actual})")]
    Hash {
        path: String,
        expected: String,
        actual: String,
    },
    #[error("{0}: invalid JSON ({1})")]
    Json(String, String),
    #[error("{0}")]
    Shape(String),
    #[error("{0}")]
    Io(String),
}

/// What the adapter needs from `manifest.json`.
#[derive(Debug, Clone)]
pub struct Manifest {
    pub dataset_version: String,
    /// path → SHA-256 (hex) of every file of the dataset except the manifest itself.
    pub files: BTreeMap<String, String>,
}

/// Parses `manifest.json` and refuses a schema version other than [`SCHEMA_VERSION`].
pub fn parse_manifest(bytes: &[u8]) -> Result<Manifest, RefdataError> {
    let v: J = serde_json::from_slice(bytes)
        .map_err(|e| RefdataError::Json("manifest.json".into(), e.to_string()))?;
    let schema_version = v
        .get("schemaVersion")
        .and_then(J::as_u64)
        .ok_or_else(|| RefdataError::Manifest("no schemaVersion".into()))?;
    if schema_version != SCHEMA_VERSION {
        return Err(RefdataError::Schema(schema_version));
    }
    let text = |k: &str| {
        v.get(k)
            .and_then(J::as_str)
            .map(str::to_string)
            .ok_or_else(|| RefdataError::Manifest(format!("no {k}")))
    };
    let files = v
        .get("files")
        .and_then(J::as_object)
        .ok_or_else(|| RefdataError::Manifest("no files".into()))?
        .iter()
        .filter_map(|(k, h)| h.as_str().map(|h| (k.clone(), h.to_ascii_lowercase())))
        .collect();
    Ok(Manifest {
        dataset_version: text("datasetVersion")?,
        files,
    })
}

/// SHA-256 (lowercase hex) of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

impl Manifest {
    /// Checks `bytes` against the SHA-256 the manifest lists for `path`.
    pub fn verify(&self, path: &str, bytes: &[u8]) -> Result<(), RefdataError> {
        let expected = self
            .files
            .get(path)
            .ok_or_else(|| RefdataError::Unlisted(path.into()))?;
        let actual = sha256_hex(bytes);
        if &actual != expected {
            return Err(RefdataError::Hash {
                path: path.into(),
                expected: expected.clone(),
                actual,
            });
        }
        Ok(())
    }
}

/// The parsed data files of one dataset release.
pub struct Dataset {
    pub manifest: Manifest,
    pub game: J,
    pub enus: J,
    pub patches: J,
    pub assets: J,
}

impl Dataset {
    /// Builds a dataset from the raw bytes of [`DATA_FILES`] (`get(path)` returns the file as listed in
    /// the manifest). Every file is checked against the manifest before it is parsed.
    pub fn from_files(
        manifest: Manifest,
        mut get: impl FnMut(&str) -> Result<Vec<u8>, RefdataError>,
    ) -> Result<Dataset, RefdataError> {
        let mut read = |path: &str| -> Result<J, RefdataError> {
            let bytes = get(path)?;
            manifest.verify(path, &bytes)?;
            serde_json::from_slice(&bytes)
                .map_err(|e| RefdataError::Json(path.into(), e.to_string()))
        };
        let game = read("game.json")?;
        let enus = read("locales/enus.json")?;
        let patches = read("patches.json")?;
        let assets = read("assets.json")?;
        Ok(Dataset {
            manifest,
            game,
            enus,
            patches,
            assets,
        })
    }
}

/// A `dim_heroes` row.
#[derive(Debug, Clone, PartialEq)]
pub struct HeroRow {
    /// Parser name: both `id` and `name` (joins with `match_players.hero`).
    pub id: String,
    pub role: Option<String>,
    pub universe: Option<String>,
    pub data: J,
}

/// A `dim_talents` row.
#[derive(Debug, Clone, PartialEq)]
pub struct TalentRow {
    pub hero_id: String,
    pub tier: i32,
    pub name: String,
    pub tree_id: String,
    pub data: J,
}

/// A `dim_patches` row; `data.detail` holds the patch detail in the old API shape.
#[derive(Debug, Clone, PartialEq)]
pub struct PatchRow {
    pub internal_id: String,
    pub name: String,
    pub patch_type: String,
    /// `YYYY-MM-DD` (cast to `timestamptz` on write).
    pub live_date: Option<String>,
    pub hero_count: i32,
    pub map_count: i32,
    pub aliases: Vec<String>,
    pub data: J,
}

/// A `patch_hero_sections` row.
#[derive(Debug, Clone, PartialEq)]
pub struct HeroSectionRow {
    pub patch_internal_id: String,
    pub anchor: String,
    /// Dataset hero key (e.g. `FaerieDragon`).
    pub hero_short_name: String,
    /// Parser name of the hero (e.g. `Brightwing`).
    pub hero_name: String,
    pub hero_key: String,
    pub classification: String,
    pub short_summary: String,
    pub content: String,
    pub patch_name: String,
    pub patch_type: String,
    pub live_date: Option<String>,
    /// `live` or `ptr` (PTR candidates are kept out of `/api/hero-changes`).
    pub channel: String,
}

/// One image to copy into `images_dir`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ImageCopy {
    /// Path inside the dataset tarball (`images/heroportraits/….png`).
    pub source: String,
    /// SHA-256 listed by `assets.json` for that file.
    pub sha256: String,
    /// Path under `images_dir` (`heroes/<slug>.png`, `battlegrounds/<slug>.png`), served on `/images`.
    pub dest: String,
}

/// Everything one dataset turns into.
#[derive(Debug, Clone)]
pub struct Rows {
    pub dataset_version: String,
    pub heroes: Vec<HeroRow>,
    pub talents: Vec<TalentRow>,
    pub patches: Vec<PatchRow>,
    pub sections: Vec<HeroSectionRow>,
    pub images: Vec<ImageCopy>,
}

/// `text` of a localized string (`{html, text}`), `None` when missing (never another locale).
fn loc_text(v: Option<&J>) -> Option<String> {
    v.and_then(|t| t.get("text"))
        .and_then(J::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn obj<'a>(v: &'a J, key: &str) -> Result<&'a Map<String, J>, RefdataError> {
    v.get(key)
        .and_then(J::as_object)
        .ok_or_else(|| RefdataError::Shape(format!("missing object `{key}`")))
}

/// Universe as the front knows it (portrait ring colours): the dataset's `franchise` uses
/// `Starcraft` and `Classic` where the front expects `StarCraft` and `Nexus`.
pub fn universe(franchise: &str) -> String {
    match franchise {
        "Starcraft" => "StarCraft".into(),
        "Classic" => "Nexus".into(),
        other => other.into(),
    }
}

/// Old API `patchType` labels (shown as-is by the front): the dataset types are lowercase enums.
pub fn patch_type_label(t: &str) -> &'static str {
    match t {
        "ptr" => "PTR",
        "hotfix" => "Hotfix Patch",
        "balance" => "Balance Update",
        _ => "Patch Notes",
    }
}

/// Image slug of a hero, as the old API named its files (`/images/heroes/<slug>.png`): the English
/// name, accents folded, lowercase alphanumeric only ("Lt. Morales" → `ltmorales`, "Lúcio" → `lucio`).
pub fn hero_slug(name: &str) -> String {
    name.chars()
        .map(fold_accent)
        .flat_map(char::to_lowercase)
        .filter(char::is_ascii_alphanumeric)
        .collect()
}

/// Image slug of a map, the rule of the dataset's `Battleground.slug` and of the front's `map_slug`
/// (`web-rs/core`): lowercase, apostrophes dropped, whitespace runs → `-`, then only `[a-z0-9-_.()]`
/// kept ("Checkpoint: Hanamura" → `checkpoint-hanamura`). Used when a release has no `slug`.
pub fn map_slug(name: &str) -> String {
    name.to_lowercase()
        .replace(['\'', '\u{2019}'], "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .filter(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.' | '(' | ')')
        })
        .collect()
}

/// An image destination is joined to the images folder, and the names it is built from come from the
/// dataset: every `/`-separated segment must be a plain file name (ASCII alphanumerics, `-`, `_`, `.`,
/// `(`, `)`; not empty, `.` or `..`), so a crafted map or hero name cannot write outside that folder.
/// A `:` is refused too: on Windows it would name an NTFS alternate data stream ("Checkpoint: Hanamura"
/// gets no map art).
pub fn safe_image_dest(dest: &str) -> bool {
    dest.split('/').all(|seg| {
        !seg.is_empty()
            && seg != "."
            && seg != ".."
            && seg
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '(' | ')'))
    })
}

/// Normalises a hero name for the patch-section join (mirror of `hero_key()` on the front side):
/// accents folded, lowercase alphanumeric only, leading "the" removed ("The Lost Vikings" → `lostvikings`).
pub fn hero_key(s: &str) -> String {
    let k = hero_slug(s);
    k.strip_prefix("the").map(str::to_string).unwrap_or(k)
}

fn fold_accent(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ä' | 'ã' | 'À' | 'Á' | 'Â' | 'Ä' | 'Ã' => 'a',
        'è' | 'é' | 'ê' | 'ë' | 'È' | 'É' | 'Ê' | 'Ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' | 'Ì' | 'Í' | 'Î' | 'Ï' => 'i',
        'ò' | 'ó' | 'ô' | 'ö' | 'õ' | 'Ò' | 'Ó' | 'Ô' | 'Ö' | 'Õ' => 'o',
        'ù' | 'ú' | 'û' | 'ü' | 'Ù' | 'Ú' | 'Û' | 'Ü' => 'u',
        'ñ' | 'Ñ' => 'n',
        'ç' | 'Ç' => 'c',
        other => other,
    }
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Turns a dataset into rows. Pure: the same dataset always gives the same rows.
pub fn build(ds: &Dataset) -> Result<Rows, RefdataError> {
    let game_heroes = obj(&ds.game, "heroes")?;
    let game_maps = obj(&ds.game, "battlegrounds")?;
    let enus_heroes = obj(&ds.enus, "heroes")?;
    let enus_talents = obj(&ds.enus, "talents")?;
    let enus_maps = obj(&ds.enus, "battlegrounds")?;
    let assets = ds
        .assets
        .as_object()
        .ok_or_else(|| RefdataError::Shape("assets.json is not an object".into()))?;
    // file name → (tarball path, sha256), to find a portrait or a loading screen by its file name.
    let by_file: HashMap<&str, (&str, &str)> = assets
        .iter()
        .filter_map(|(k, a)| {
            let path = a.get("path")?.as_str()?;
            let sha = a.get("sha256")?.as_str()?;
            Some((k.rsplit('/').next().unwrap_or(k), (path, sha)))
        })
        .collect();
    let image = |file: Option<&str>, dest: String| -> Option<ImageCopy> {
        if !safe_image_dest(&dest) {
            tracing::warn!("reference data: image destination {dest:?} refused (unsafe path)");
            return None;
        }
        let (path, sha) = by_file.get(file?)?;
        Some(ImageCopy {
            source: (*path).to_string(),
            sha256: sha.to_ascii_lowercase(),
            dest,
        })
    };
    let attr = storm_stats::constants::hero_attribute();

    let mut heroes = Vec::new();
    let mut talents = Vec::new();
    let mut images = BTreeSet::new();
    // dataset hero key → parser name (for the hero sections of the patch notes).
    let mut canonical: HashMap<&str, String> = HashMap::new();
    for (key, h) in game_heroes {
        let s = h.get("structure").unwrap_or(&J::Null);
        let loc = enus_heroes.get(key);
        let english = loc_text(loc.and_then(|l| l.get("name")));
        let attribute_id = h
            .get("attributeId")
            .or_else(|| s.get("attributeId"))
            .and_then(J::as_str);
        let id = attribute_id
            .and_then(|a| attr.get(a))
            .and_then(J::as_str)
            .map(str::to_string)
            .or_else(|| english.clone())
            .unwrap_or_else(|| key.clone());
        let slug = hero_slug(english.as_deref().unwrap_or(key));
        let portraits = s.get("portraits");
        // D3: the square `heroSelect` icon when the dataset bundles it, else the loading portrait.
        let portrait = ["heroSelect", "loading"].iter().find_map(|p| {
            image(
                portraits.and_then(|v| v.get(*p)).and_then(J::as_str),
                format!("heroes/{slug}.png"),
            )
        });
        if let Some(p) = portrait {
            images.insert(p);
        }
        let role = loc
            .and_then(|l| l.get("roles"))
            .and_then(J::as_array)
            .and_then(|r| loc_text(r.first()));
        let franchise = s.get("franchise").and_then(J::as_str);
        heroes.push(HeroRow {
            id: id.clone(),
            role: role.clone(),
            universe: franchise.map(universe),
            data: json!({
                "key": key,
                "attributeId": attribute_id,
                "shortName": slug,
                "name": english,
                "icon": format!("/images/heroes/{slug}.png"),
                "role": role,
                "expandedRole": loc_text(loc.and_then(|l| l.get("expandedRole"))),
                "title": loc_text(loc.and_then(|l| l.get("title"))),
                "franchise": franchise,
            }),
        });
        if let Some(levels) = s.get("talents").and_then(J::as_object) {
            for (level, list) in levels {
                let Some(tier) = level
                    .strip_prefix("Level")
                    .and_then(|n| n.parse::<i32>().ok())
                else {
                    continue;
                };
                for t in list.as_array().into_iter().flatten() {
                    let Some(tree_id) = t.get("talentId").and_then(J::as_str) else {
                        continue;
                    };
                    let tl = t
                        .get("linkId")
                        .and_then(J::as_str)
                        .and_then(|l| enus_talents.get(l));
                    talents.push(TalentRow {
                        hero_id: id.clone(),
                        tier,
                        name: loc_text(tl.and_then(|l| l.get("name")))
                            .unwrap_or_else(|| tree_id.to_string()),
                        tree_id: tree_id.to_string(),
                        data: json!({
                            "icon": t.get("icon"),
                            "type": t.get("abilityType"),
                            "description": loc_text(tl.and_then(|l| l.get("fullText"))),
                            "sort": t.get("sort"),
                            "isQuest": t.get("isQuest"),
                        }),
                    });
                }
            }
        }
        canonical.insert(key.as_str(), id);
    }
    heroes.sort_by(|a, b| a.id.cmp(&b.id));
    talents.sort_by(|a, b| (&a.hero_id, a.tier, &a.tree_id).cmp(&(&b.hero_id, b.tier, &b.tree_id)));

    for (key, m) in game_maps {
        let english = loc_text(enus_maps.get(key).and_then(|l| l.get("name")));
        let Some(name) = english else { continue };
        let art = m
            .get("structure")
            .and_then(|s| s.get("loadingScreenImage"))
            .and_then(J::as_str);
        // The release's `slug` (HotsPatchNotes, same rule) wins; older releases have none.
        let slug = m
            .get("slug")
            .and_then(J::as_str)
            .map_or_else(|| map_slug(&name), str::to_string);
        if let Some(img) = image(art, format!("battlegrounds/{slug}.png")) {
            images.insert(img);
        }
    }

    let list = ds
        .patches
        .as_array()
        .ok_or_else(|| RefdataError::Shape("patches.json is not an array".into()))?;
    let mut patches = Vec::new();
    let mut sections = Vec::new();
    for p in list {
        let (row, secs) = build_patch(p, &canonical)?;
        patches.push(row);
        sections.extend(secs);
    }

    Ok(Rows {
        dataset_version: ds.manifest.dataset_version.clone(),
        heroes,
        talents,
        patches,
        sections,
        images: images.into_iter().collect(),
    })
}

/// One dataset patch → its `dim_patches` row (with the detail in the old API shape) and its hero sections.
fn build_patch(
    p: &J,
    canonical: &HashMap<&str, String>,
) -> Result<(PatchRow, Vec<HeroSectionRow>), RefdataError> {
    let s = |k: &str| p.get(k).and_then(J::as_str);
    let id = s("id")
        .ok_or_else(|| RefdataError::Shape("patch without id".into()))?
        .to_string();
    let name = s("name").unwrap_or(&id).to_string();
    let patch_type = patch_type_label(s("patchType").unwrap_or("")).to_string();
    let channel = s("channel").unwrap_or("live").to_string();
    let date = s("date").filter(|d| !d.is_empty()).map(str::to_string);
    let live_date_json = date.as_ref().map(|d| format!("{d}T00:00:00"));
    let availability = s("availability").unwrap_or("available");
    let aliases: Vec<String> = p
        .get("aliases")
        .and_then(J::as_array)
        .map(|a| a.iter().filter_map(J::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    let sources = p
        .get("sources")
        .and_then(J::as_array)
        .cloned()
        .unwrap_or_default();
    fn authority(x: &J) -> &str {
        x.get("authority").and_then(J::as_str).unwrap_or("")
    }
    let official_link = sources
        .iter()
        .find(|x| authority(x) == "official")
        .or_else(|| {
            sources
                .iter()
                .find(|x| authority(x).starts_with("official"))
        })
        .and_then(|x| x.get("url"))
        .and_then(J::as_str);
    let community_only = !sources.is_empty() && sources.iter().all(|x| authority(x) == "community");
    let has_content = availability == "available";

    let content = p.get("contents").and_then(|c| c.get("enus"));
    let secs: Vec<&J> = content
        .and_then(|c| c.get("sections"))
        .and_then(J::as_array)
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    let mut toc = Vec::new();
    let mut detail_sections = Vec::new();
    let mut hero_rows = Vec::new();
    let mut heroes = BTreeSet::new();
    let mut maps = BTreeSet::new();
    for (order, sec) in secs.iter().enumerate() {
        let g = |k: &str| sec.get(k).and_then(J::as_str);
        let anchor = g("id").unwrap_or_default().to_string();
        let heading = g("heading").unwrap_or_default().to_string();
        let level = sec.get("level").and_then(J::as_i64).unwrap_or(0);
        let hero = g("heroId").filter(|h| !h.is_empty());
        let map = g("battlegroundId").filter(|m| !m.is_empty());
        let section_type = match (hero, map) {
            (Some(_), _) => "Hero",
            (None, Some(_)) => "Battleground",
            _ => "General",
        };
        let verdict = classify::classify(g("text").unwrap_or_default());
        let html = g("html").unwrap_or_default().to_string();
        toc.push(json!({
            "order": order, "headingLevel": level, "title": heading,
            "anchor": anchor, "sectionType": section_type,
        }));
        detail_sections.push(json!({
            "classification": verdict.classification, "shortSummary": verdict.short_summary,
            "order": order, "headingLevel": level, "sectionType": section_type,
            "entityName": heading, "anchor": anchor, "heroShortName": hero, "content": html,
        }));
        if let Some(m) = map {
            maps.insert(m.to_string());
        }
        if let Some(h) = hero {
            heroes.insert(h.to_string());
            if has_content {
                let hero_name = canonical.get(h).cloned().unwrap_or_else(|| heading.clone());
                hero_rows.push(HeroSectionRow {
                    patch_internal_id: id.clone(),
                    anchor: anchor.clone(),
                    hero_short_name: h.to_string(),
                    hero_key: hero_key(&hero_name),
                    hero_name,
                    classification: verdict.classification.to_string(),
                    short_summary: verdict.short_summary,
                    content: html,
                    patch_name: name.clone(),
                    patch_type: patch_type.clone(),
                    live_date: date.clone(),
                    channel: channel.clone(),
                });
            }
        }
    }
    let article = content
        .and_then(|c| c.get("html"))
        .and_then(J::as_str)
        .unwrap_or_default();
    let body = anchored_html(article, &secs);

    let detail = json!({
        "internalId": id, "patchName": name, "patchType": patch_type, "liveDate": live_date_json,
        "officialLink": official_link, "content": body, "tableOfContents": toc,
        "sections": detail_sections, "aliases": aliases, "channel": channel,
        "availability": availability, "hasContent": has_content,
        "communityOnly": community_only, "reason": p.get("reason"),
    });
    let data = json!({
        "officialLink": official_link, "hasContent": has_content, "channel": channel,
        "availability": availability, "communityOnly": community_only, "reason": p.get("reason"),
        "build": p.get("build"), "sources": sources, "detail": detail,
    });
    Ok((
        PatchRow {
            internal_id: id,
            name,
            patch_type,
            live_date: date,
            hero_count: i32::try_from(heroes.len()).unwrap_or(i32::MAX),
            map_count: i32::try_from(maps.len()).unwrap_or(i32::MAX),
            aliases,
            data,
        },
        hero_rows,
    ))
}

/// The article HTML with each section's `id` set on its heading, so the table of contents and the
/// `/patch/<id>#<anchor>` links scroll to it. The article's headings match the sections by position,
/// counted from the end (the dataset may add one leading section for the text before the first heading).
/// When they do not line up, the content is rebuilt from the sections (headings + section HTML).
pub fn anchored_html(article: &str, sections: &[&J]) -> String {
    static HEADING: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = HEADING.get_or_init(|| {
        regex::Regex::new(r"<h([1-6])(\s[^>]*)?>").unwrap_or_else(|e| panic!("heading regex: {e}"))
    });
    let level = |s: &J| s.get("level").and_then(J::as_i64).unwrap_or(0);
    let id = |s: &J| {
        s.get("id")
            .and_then(J::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let tags: Vec<regex::Captures> = re.captures_iter(article).collect();
    let aligned = tags.len() <= sections.len() && {
        let skip = sections.len() - tags.len();
        tags.iter()
            .zip(&sections[skip..])
            .all(|(t, s)| t[1].parse::<i64>().ok() == Some(level(s)))
    };
    if aligned && !tags.is_empty() {
        let skip = sections.len() - tags.len();
        let mut out = String::with_capacity(article.len() + tags.len() * 24);
        let mut last = 0;
        for (t, s) in tags.iter().zip(&sections[skip..]) {
            let Some(m) = t.get(0) else { continue };
            out.push_str(&article[last..m.start()]);
            out.push_str(&format!(
                "<h{} id=\"{}\"{}>",
                &t[1],
                escape_html(&id(s)),
                t.get(2).map_or("", |a| a.as_str())
            ));
            last = m.end();
        }
        out.push_str(&article[last..]);
        return out;
    }
    if tags.is_empty() && sections.len() <= 1 {
        return article.to_string(); // no heading at all: the article as published
    }
    sections
        .iter()
        .map(|s| {
            let lv = level(s).clamp(1, 6);
            format!(
                "<h{lv} id=\"{}\">{}</h{lv}>\n{}\n",
                escape_html(&id(s)),
                escape_html(s.get("heading").and_then(J::as_str).unwrap_or_default()),
                s.get("html").and_then(J::as_str).unwrap_or_default()
            )
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests;
