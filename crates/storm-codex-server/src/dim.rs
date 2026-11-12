//! Hero/talent reference data (`dim_*`) replicated from the HotsPatchNotes API at startup —
//! single source (no second sync pipeline). Best-effort: an outage does not prevent the
//! server from starting (the front has a colour fallback). Config `HOTSPATCHNOTES_URL`.

use sqlx::PgPool;
use std::path::{Path, PathBuf};

/// Replicates the heroes from `<base>/api/heroes` into `dim_heroes`. Key = `name` (storm-stats
/// uses the hero names). Best-effort.
pub async fn sync_heroes(db: &PgPool, base_url: &str) {
    let url = format!("{}/api/heroes", base_url.trim_end_matches('/'));
    let fetched = tokio::task::spawn_blocking(move || -> Result<serde_json::Value, String> {
        let body = ureq::get(&url)
            .call()
            .map_err(|e| e.to_string())?
            .body_mut()
            .read_to_string()
            .map_err(|e| e.to_string())?;
        serde_json::from_str(&body).map_err(|e| e.to_string())
    })
    .await;

    let heroes = match fetched {
        Ok(Ok(serde_json::Value::Array(h))) => h,
        Ok(Ok(_)) => {
            tracing::warn!("dim_heroes: unexpected response from HotsPatchNotes");
            return;
        }
        Ok(Err(e)) => {
            tracing::warn!(
                "dim_heroes: HotsPatchNotes unavailable ({e}) — universe rings fall back"
            );
            return;
        }
        Err(_) => return, // join error
    };

    let mut n = 0;
    for h in &heroes {
        let name = h.get("name").and_then(|v| v.as_str());
        let Some(name) = name else { continue };
        let role = h.get("role").and_then(|v| v.as_str());
        let universe = h.get("universe").and_then(|v| v.as_str());
        if let Err(e) = sqlx::query(
            "INSERT INTO dim_heroes (id, name, role, universe, data)
             VALUES ($1,$2,$3,$4,$5)
             ON CONFLICT (id) DO UPDATE SET name=EXCLUDED.name, role=EXCLUDED.role,
                universe=EXCLUDED.universe, data=EXCLUDED.data",
        )
        .bind(name) // id = name (join key with storm-stats)
        .bind(name)
        .bind(role)
        .bind(universe)
        .bind(h)
        .execute(db)
        .await
        {
            tracing::warn!("reference data: write failed: {e}");
        }
        n += 1;
    }
    tracing::info!("dim_heroes synced: {n} heroes from HotsPatchNotes");
}

/// Replicates the LIST of patch notes from `<base>/api/patches` into `dim_patches` (storm-codex
/// becomes their owner). Returns the NEW patches (missing before) → the caller notifies them.
/// No "new" ones on the very first seed (empty table) to avoid 200 notifications. Best-effort.
pub async fn sync_patches(db: &PgPool, base_url: &str) -> Vec<(String, String)> {
    let url = format!(
        "{}/api/patches?page=1&pageSize=200",
        base_url.trim_end_matches('/')
    );
    let items = match tokio::task::spawn_blocking(move || fetch_json(&url)).await {
        Ok(Ok(v)) => v
            .get("items")
            .and_then(|i| i.as_array())
            .cloned()
            .unwrap_or_default(),
        _ => {
            tracing::warn!("dim_patches: HotsPatchNotes unavailable");
            return Vec::new();
        }
    };
    let was_empty: i64 = sqlx::query_scalar("SELECT count(*) FROM dim_patches")
        .fetch_one(db)
        .await
        .unwrap_or(0);
    let mut new_patches: Vec<(String, String)> = Vec::new();
    for it in &items {
        let Some(iid) = it.get("internalId").and_then(|v| v.as_str()) else {
            continue;
        };
        let name = it.get("patchName").and_then(|v| v.as_str()).unwrap_or(iid);
        let inserted: Option<bool> = sqlx::query_scalar(
            "INSERT INTO dim_patches (internal_id, name, type, live_date, hero_count, map_count, data)
             VALUES ($1,$2,$3,$4::timestamptz,$5,$6,$7)
             ON CONFLICT (internal_id) DO UPDATE SET name=EXCLUDED.name, type=EXCLUDED.type,
                live_date=EXCLUDED.live_date, hero_count=EXCLUDED.hero_count,
                map_count=EXCLUDED.map_count, data=EXCLUDED.data
             RETURNING (xmax = 0)",
        )
        .bind(iid)
        .bind(name)
        .bind(it.get("patchType").and_then(|v| v.as_str()))
        .bind(it.get("liveDate").and_then(|v| v.as_str()))
        .bind(it.get("heroCount").and_then(|v| v.as_i64()).map(|v| v as i32))
        .bind(it.get("mapCount").and_then(|v| v.as_i64()).map(|v| v as i32))
        .bind(it)
        .fetch_optional(db)
        .await
        .ok()
        .flatten();
        if inserted == Some(true) {
            new_patches.push((iid.to_string(), name.to_string()));
        }
    }
    tracing::info!(
        "dim_patches synced: {} patches ({} new)",
        items.len(),
        new_patches.len()
    );
    if was_empty == 0 {
        Vec::new()
    } else {
        new_patches
    } // initial seed → no notification
}

/// Replicates the talents per hero from `<base>/api/heroes/{shortName}` into `dim_talents`.
/// Join key `tree_id` = `talentTreeId`, which matches `player.talents[TierNChoice]` written by
/// the parser. ~90 sequential (blocking) requests, run as a background task; full refresh
/// (DELETE + INSERT in a transaction). Best-effort: an outage prevents nothing (the front has a text
/// fallback on the raw id). `tier` = hero level (1,4,7,10,13,16,20), `hero_id` = name (metadata).
pub async fn sync_talents(db: &PgPool, base_url: &str) {
    let base = base_url.trim_end_matches('/').to_string();
    let rows = match tokio::task::spawn_blocking(move || collect_talents(&base)).await {
        Ok(r) if !r.is_empty() => r,
        Ok(_) => {
            tracing::warn!("dim_talents: no talent collected (HotsPatchNotes unavailable?)");
            return;
        }
        Err(_) => return, // join error
    };

    let mut tx = match db.begin().await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("dim_talents: begin failed ({e})");
            return;
        }
    };
    if sqlx::query("DELETE FROM dim_talents")
        .execute(&mut *tx)
        .await
        .is_err()
    {
        return;
    }
    let mut n = 0;
    for (hero_id, tier, name, tree_id, data) in &rows {
        let r = sqlx::query(
            "INSERT INTO dim_talents (hero_id, tier, name, tree_id, data)
             VALUES ($1,$2,$3,$4,$5)
             ON CONFLICT (hero_id, tier, name) DO NOTHING",
        )
        .bind(hero_id)
        .bind(tier)
        .bind(name)
        .bind(tree_id)
        .bind(data)
        .execute(&mut *tx)
        .await;
        if r.is_ok() {
            n += 1;
        }
    }
    if tx.commit().await.is_ok() {
        tracing::info!("dim_talents synced: {n} talents from HotsPatchNotes");
    }
}

/// Blocking collection: hero list → detail per `shortName` → flattened talents.
/// Tuple = (hero_id, tier=niveau, name, tree_id=talentTreeId, data{icon,type,description}).
/// `hero_id` = **canonical parser name** (via the HotsPatchNotes `attributeId` → storm-stats'
/// `attr.json`), not the HotsPatchNotes name: they differ in punctuation/articles ("Li-Ming" vs
/// "LiMing", "The Lost Vikings" vs "LostVikings"). Aligning it on the parser avoids false
/// corrections when resolving the played hero (see project.rs::talent_hero).
fn collect_talents(base: &str) -> Vec<(String, i32, String, String, serde_json::Value)> {
    let mut out = Vec::new();
    // attribute code (4 letters) → canonical parser name (frozen hots-parser table).
    let code_to_name: std::collections::HashMap<String, String> =
        storm_stats::constants::hero_attribute()
            .as_object()
            .map(|o| {
                o.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();
    let Ok(list) = fetch_array(&format!("{base}/api/heroes")) else {
        return out;
    };
    for h in &list {
        let Some(short) = h.get("shortName").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(hp_name) = h.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        let Ok(detail) = fetch_json(&format!("{base}/api/heroes/{short}")) else {
            continue;
        };
        // attributeId HotsPatchNotes == code attr.json (ex. Johanna="Crus", E.T.C.="L90E") →
        // canonical parser name; fallback on the HotsPatchNotes name if missing/not mapped.
        let hero_name = detail
            .get("attributeId")
            .and_then(|v| v.as_str())
            .and_then(|a| code_to_name.get(a))
            .map(String::as_str)
            .unwrap_or(hp_name);
        let Some(talents) = detail.get("talents").and_then(|v| v.as_object()) else {
            continue;
        };
        for (level_key, arr) in talents {
            let tier: i32 = level_key.parse().unwrap_or(0);
            let Some(arr) = arr.as_array() else { continue };
            for t in arr {
                let Some(tree_id) = t.get("talentTreeId").and_then(|v| v.as_str()) else {
                    continue;
                };
                let name = t.get("name").and_then(|v| v.as_str()).unwrap_or(tree_id);
                let data = serde_json::json!({
                    "icon": t.get("icon"),
                    "type": t.get("type"),
                    "description": t.get("description"),
                    "sort": t.get("sort"),
                    "isQuest": t.get("isQuest"),
                });
                out.push((
                    hero_name.to_string(),
                    tier,
                    name.to_string(),
                    tree_id.to_string(),
                    data,
                ));
            }
        }
    }
    out
}

/// Vendors (once) the hero portraits + map images from HotsPatchNotes into
/// `images_dir` (served on `/images`). Idempotent: skips the files already present. Best-effort
/// — an outage prevents nothing (the front has a fallback). Makes storm-codex self-sufficient.
pub async fn vendor_images(images_dir: &Path, base_url: &str) {
    let base = base_url.trim_end_matches('/').to_string();
    let dir = images_dir.to_path_buf();
    let _ = tokio::task::spawn_blocking(move || {
        let mut got = 0;
        // /api/heroes → "icon" field ("/images/heroes/<slug>.png")
        if let Ok(list) = fetch_array(&format!("{base}/api/heroes")) {
            got += download_referenced(&list, "icon", &base, &dir);
        }
        // /api/battlegrounds → "imageUrl" field ("/images/battlegrounds/<slug>.png")
        if let Ok(list) = fetch_array(&format!("{base}/api/battlegrounds")) {
            got += download_referenced(&list, "imageUrl", &base, &dir);
        }
        tracing::info!("images vendored: {got} new files in {dir:?}");
    })
    .await;
}

/// Ingestion of the **reference-data snapshot** (standalone bundle mode): downloads `referential.tar.gz`
/// (`REFERENTIAL_URL`), unpacks it, and populates `dim_heroes` / `dim_talents` / `dim_patches`
/// (+ patch detail merged into `data.detail` for offline reading) then vendors the images into
/// `images_dir`. No runtime dependency on HotsPatchNotes. Returns the NEW patches (for
/// notification), like `sync_patches`; no "new" ones on the first seed (empty table). Best-effort.
pub async fn ingest_snapshot(db: &PgPool, images_dir: &Path, url: &str) -> Vec<(String, String)> {
    let url = url.to_string();
    let tmp = unique_tmp_dir();

    // Download + unpacking (blocking: network + tar/gzip).
    let prep = tokio::task::spawn_blocking({
        let tmp = tmp.clone();
        move || -> Result<(), String> {
            let bytes = ureq::get(&url)
                .call()
                .map_err(|e| e.to_string())?
                .body_mut()
                .read_to_vec()
                .map_err(|e| e.to_string())?;
            unpack_snapshot(&bytes, &tmp)
        }
    })
    .await;
    ingest_unpacked(db, images_dir, tmp, prep).await
}

/// Same as [`ingest_snapshot`] but reads the `referential.tar.gz` from disk (standalone bundle:
/// the installer ships the snapshot, no network). Best-effort: a missing/corrupt file logs a
/// warning and returns an empty list.
pub async fn ingest_snapshot_file(
    db: &PgPool,
    images_dir: &Path,
    file: &Path,
) -> Vec<(String, String)> {
    let file = file.to_path_buf();
    let tmp = unique_tmp_dir();
    let prep = tokio::task::spawn_blocking({
        let tmp = tmp.clone();
        move || -> Result<(), String> {
            let bytes = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            unpack_snapshot(&bytes, &tmp)
        }
    })
    .await;
    ingest_unpacked(db, images_dir, tmp, prep).await
}

/// One scratch directory per call so two overlapping ingests never clobber each other.
fn unique_tmp_dir() -> PathBuf {
    std::env::temp_dir().join(format!("storm-codex-referential-{}", uuid::Uuid::new_v4()))
}

/// Unpacks a gzipped tarball into a fresh `tmp`.
fn unpack_snapshot(bytes: &[u8], tmp: &Path) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(tmp);
    std::fs::create_dir_all(tmp).map_err(|e| e.to_string())?;
    let gz = flate2::read::GzDecoder::new(bytes);
    tar::Archive::new(gz).unpack(tmp).map_err(|e| e.to_string())
}

/// Shared tail of both entry points: checks the preparation outcome, ingests the unpacked
/// snapshot in `tmp` (removed afterwards, also on failure).
async fn ingest_unpacked(
    db: &PgPool,
    images_dir: &Path,
    tmp: PathBuf,
    prep: Result<Result<(), String>, tokio::task::JoinError>,
) -> Vec<(String, String)> {
    let dir = images_dir.to_path_buf();
    match prep {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            tracing::warn!("reference data: snapshot unavailable ({e}) — universe rings fall back");
            let _ = std::fs::remove_dir_all(&tmp);
            return Vec::new();
        }
        Err(_) => return Vec::new(),
    }

    let read = |name: &str| -> Option<serde_json::Value> {
        std::fs::read_to_string(tmp.join(name))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
    };

    // dim_heroes (same schema as sync_heroes: id = name).
    if let Some(serde_json::Value::Array(heroes)) = read("heroes.json") {
        let mut n = 0;
        for h in &heroes {
            let Some(name) = h.get("name").and_then(|v| v.as_str()) else {
                continue;
            };
            if let Err(e) = sqlx::query(
                "INSERT INTO dim_heroes (id, name, role, universe, data)
                 VALUES ($1,$2,$3,$4,$5)
                 ON CONFLICT (id) DO UPDATE SET name=EXCLUDED.name, role=EXCLUDED.role,
                    universe=EXCLUDED.universe, data=EXCLUDED.data",
            )
            .bind(name)
            .bind(name)
            .bind(h.get("role").and_then(|v| v.as_str()))
            .bind(h.get("universe").and_then(|v| v.as_str()))
            .bind(h)
            .execute(db)
            .await
            {
                tracing::warn!("reference data: write failed: {e}");
            }
            n += 1;
        }
        tracing::info!("dim_heroes (snapshot): {n} heroes");
    }

    // dim_talents from hero-details.json ({shortName: detail}). Same canonical mapping as
    // collect_talents: HotsPatchNotes attributeId → parser name (attr.json); fallback HPN name.
    if let Some(serde_json::Value::Object(details)) = read("hero-details.json") {
        let code_to_name: std::collections::HashMap<String, String> =
            storm_stats::constants::hero_attribute()
                .as_object()
                .map(|o| {
                    o.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect()
                })
                .unwrap_or_default();
        if let Ok(mut tx) = db.begin().await {
            if sqlx::query("DELETE FROM dim_talents")
                .execute(&mut *tx)
                .await
                .is_ok()
            {
                let mut n = 0;
                for detail in details.values() {
                    let hp_name = detail.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let hero_name = detail
                        .get("attributeId")
                        .and_then(|v| v.as_str())
                        .and_then(|a| code_to_name.get(a))
                        .map(String::as_str)
                        .unwrap_or(hp_name);
                    let Some(talents) = detail.get("talents").and_then(|v| v.as_object()) else {
                        continue;
                    };
                    for (level_key, arr) in talents {
                        let tier: i32 = level_key.parse().unwrap_or(0);
                        let Some(arr) = arr.as_array() else { continue };
                        for t in arr {
                            let Some(tree_id) = t.get("talentTreeId").and_then(|v| v.as_str())
                            else {
                                continue;
                            };
                            let name = t.get("name").and_then(|v| v.as_str()).unwrap_or(tree_id);
                            let data = serde_json::json!({
                                "icon": t.get("icon"), "type": t.get("type"),
                                "description": t.get("description"), "sort": t.get("sort"),
                                "isQuest": t.get("isQuest"),
                            });
                            let r = sqlx::query(
                                "INSERT INTO dim_talents (hero_id, tier, name, tree_id, data)
                                 VALUES ($1,$2,$3,$4,$5)
                                 ON CONFLICT (hero_id, tier, name) DO NOTHING",
                            )
                            .bind(hero_name)
                            .bind(tier)
                            .bind(name)
                            .bind(tree_id)
                            .bind(&data)
                            .execute(&mut *tx)
                            .await;
                            if r.is_ok() {
                                n += 1;
                            }
                        }
                    }
                }
                if tx.commit().await.is_ok() {
                    tracing::info!("dim_talents (snapshot): {n} talents");
                }
            }
        }
    }

    // dim_patches: items + merged detail (data.detail) for the offline rendering of the patch.
    let mut new_patches: Vec<(String, String)> = Vec::new();
    if let Some(items) =
        read("patches.json").and_then(|v| v.get("items").and_then(|i| i.as_array()).cloned())
    {
        let pdetails = read("patch-details.json").unwrap_or(serde_json::Value::Null);
        let was_empty: i64 = sqlx::query_scalar("SELECT count(*) FROM dim_patches")
            .fetch_one(db)
            .await
            .unwrap_or(0);
        for it in &items {
            let Some(iid) = it.get("internalId").and_then(|v| v.as_str()) else {
                continue;
            };
            let name = it.get("patchName").and_then(|v| v.as_str()).unwrap_or(iid);
            let mut data = it.clone();
            if let Some(d) = pdetails.get(iid) {
                data["detail"] = d.clone();
            }
            let inserted: Option<bool> = sqlx::query_scalar(
                "INSERT INTO dim_patches (internal_id, name, type, live_date, hero_count, map_count, data)
                 VALUES ($1,$2,$3,$4::timestamptz,$5,$6,$7)
                 ON CONFLICT (internal_id) DO UPDATE SET name=EXCLUDED.name, type=EXCLUDED.type,
                    live_date=EXCLUDED.live_date, hero_count=EXCLUDED.hero_count,
                    map_count=EXCLUDED.map_count, data=EXCLUDED.data
                 RETURNING (xmax = 0)",
            )
            .bind(iid)
            .bind(name)
            .bind(it.get("patchType").and_then(|v| v.as_str()))
            .bind(it.get("liveDate").and_then(|v| v.as_str()))
            .bind(it.get("heroCount").and_then(|v| v.as_i64()).map(|v| v as i32))
            .bind(it.get("mapCount").and_then(|v| v.as_i64()).map(|v| v as i32))
            .bind(&data)
            .fetch_optional(db)
            .await
            .ok()
            .flatten();
            if inserted == Some(true) {
                new_patches.push((iid.to_string(), name.to_string()));
            }
            // Projection of the hero sections (hero ↔ patch direction) — the detail is already at hand.
            if let Some(detail) = pdetails.get(iid) {
                project_patch_sections(
                    db,
                    iid,
                    name,
                    it.get("patchType").and_then(|v| v.as_str()),
                    it.get("liveDate").and_then(|v| v.as_str()),
                    detail,
                )
                .await;
            }
        }
        tracing::info!(
            "dim_patches (snapshot): {} patches ({} new)",
            items.len(),
            new_patches.len()
        );
        if was_empty == 0 {
            new_patches.clear(); // initial seed → no notification
        }
    }

    // Images: copy images/** of the snapshot to images_dir (idempotent, skips what exists).
    let img_src = tmp.join("images");
    let _ = tokio::task::spawn_blocking(move || {
        let got = copy_tree(&img_src, &dir);
        let _ = std::fs::remove_dir_all(&tmp);
        tracing::info!("images (snapshot): {got} files copied to {dir:?}");
    })
    .await;

    new_patches
}

/// Recursive copy `src` → `dst` (creates the folders, skips the files already present). Returns the
/// number of files written. Best-effort (per-file errors are ignored).
fn copy_tree(src: &Path, dst: &Path) -> u32 {
    let mut n = 0;
    let Ok(entries) = std::fs::read_dir(src) else {
        return 0;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name() else {
            continue;
        };
        let target = dst.join(name);
        if path.is_dir() {
            let _ = std::fs::create_dir_all(&target);
            n += copy_tree(&path, &target);
        } else if !target.exists() {
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::copy(&path, &target).is_ok() {
                n += 1;
            }
        }
    }
    n
}

/// Normalises a hero name for the join (mirror of `heroKey()` on the front side): accent folding,
/// lowercase, alphanumeric only, leading "the" removed. Self-consistent (insert + query on the Rust side).
pub fn hero_key(s: &str) -> String {
    let folded: String = s.chars().map(fold_accent).collect();
    let k: String = folded
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    k.strip_prefix("the").map(|r| r.to_string()).unwrap_or(k)
}

fn fold_accent(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ä' | 'ã' => 'a',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ò' | 'ó' | 'ô' | 'ö' | 'õ' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ñ' => 'n',
        'ç' => 'c',
        other => other,
    }
}

/// Projects the `Hero`-type sections of a patch into `patch_hero_sections` (hero ↔ patch direction).
/// Full refresh for this patch (DELETE + INSERT). Best-effort. `detail` = `/api/patches/{id}` response.
pub async fn project_patch_sections(
    db: &PgPool,
    internal_id: &str,
    patch_name: &str,
    patch_type: Option<&str>,
    live_date: Option<&str>,
    detail: &serde_json::Value,
) {
    let Some(sections) = detail.get("sections").and_then(|v| v.as_array()) else {
        return;
    };
    if let Err(e) = sqlx::query("DELETE FROM patch_hero_sections WHERE patch_internal_id=$1")
        .bind(internal_id)
        .execute(db)
        .await
    {
        tracing::warn!("reference data: write failed: {e}");
    }
    for s in sections {
        if s.get("sectionType").and_then(|v| v.as_str()) != Some("Hero") {
            continue;
        }
        let Some(entity) = s
            .get("entityName")
            .and_then(|v| v.as_str())
            .filter(|e| !e.is_empty())
        else {
            continue;
        };
        let Some(anchor) = s.get("anchor").and_then(|v| v.as_str()) else {
            continue;
        };
        if let Err(e) = sqlx::query(
            "INSERT INTO patch_hero_sections
               (patch_internal_id, anchor, hero_short_name, hero_name, hero_key,
                classification, short_summary, content, patch_name, patch_type, live_date)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11::timestamptz)
             ON CONFLICT (patch_internal_id, anchor) DO UPDATE SET
                hero_short_name=EXCLUDED.hero_short_name, hero_name=EXCLUDED.hero_name,
                hero_key=EXCLUDED.hero_key, classification=EXCLUDED.classification,
                short_summary=EXCLUDED.short_summary, content=EXCLUDED.content,
                patch_name=EXCLUDED.patch_name, patch_type=EXCLUDED.patch_type, live_date=EXCLUDED.live_date",
        )
        .bind(internal_id)
        .bind(anchor)
        .bind(s.get("heroShortName").and_then(|v| v.as_str()))
        .bind(entity)
        .bind(hero_key(entity))
        .bind(s.get("classification").and_then(|v| v.as_str()))
        .bind(s.get("shortSummary").and_then(|v| v.as_str()))
        .bind(s.get("content").and_then(|v| v.as_str()))
        .bind(patch_name)
        .bind(patch_type)
        .bind(live_date)
        .execute(db)
        .await {
tracing::warn!("reference data: write failed: {e}");
}
    }
}

/// Backfill (live HPN): for each patch of `dim_patches` without projected hero sections, fetches
/// the detail from HotsPatchNotes and projects it. HPN is local → fast; idempotent, best-effort.
pub async fn backfill_hero_sections(db: &PgPool, base_url: &str) {
    let base = base_url.trim_end_matches('/').to_string();
    let pending: Vec<(String, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT internal_id, name, type, live_date::text FROM dim_patches d
         WHERE NOT EXISTS (SELECT 1 FROM patch_hero_sections p WHERE p.patch_internal_id = d.internal_id)",
    )
    .fetch_all(db)
    .await
    .unwrap_or_default();
    if pending.is_empty() {
        return;
    }
    let mut done = 0;
    for (iid, name, ptype, ldate) in &pending {
        let url = format!("{base}/api/patches/{iid}");
        let detail = tokio::task::spawn_blocking(move || fetch_json(&url)).await;
        if let Ok(Ok(detail)) = detail {
            project_patch_sections(db, iid, name, ptype.as_deref(), ldate.as_deref(), &detail)
                .await;
            done += 1;
        }
    }
    tracing::info!(
        "patch_hero_sections: backfill of {done}/{} patch(es)",
        pending.len()
    );
}

fn fetch_array(url: &str) -> Result<Vec<serde_json::Value>, String> {
    let body = ureq::get(url)
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    match serde_json::from_str(&body) {
        Ok(serde_json::Value::Array(a)) => Ok(a),
        _ => Err("non-array response".into()),
    }
}

fn fetch_json(url: &str) -> Result<serde_json::Value, String> {
    let body = ureq::get(url)
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&body).map_err(|e| e.to_string())
}

/// For each object, reads `field` (path `/images/...`), downloads `<base><path>` to
/// `<images_dir><path without /images>` if missing. Returns the number of files downloaded.
fn download_referenced(
    list: &[serde_json::Value],
    field: &str,
    base: &str,
    images_dir: &Path,
) -> u32 {
    let mut n = 0;
    for obj in list {
        let Some(path) = obj.get(field).and_then(|v| v.as_str()) else {
            continue;
        };
        // expected: "/images/heroes/x.png" or "/images/battlegrounds/x.png"
        let Some(rel) = path.strip_prefix("/images/") else {
            continue;
        };
        let dest: PathBuf = images_dir.join(rel);
        if dest.exists() {
            continue;
        }
        if let Some(parent) = dest.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let url = format!("{base}{path}");
        // 404 / empty body (e.g. ARAM map without an image) → skip, the front has a fallback
        match ureq::get(&url)
            .call()
            .and_then(|mut r| r.body_mut().read_to_vec())
        {
            Ok(bytes) if !bytes.is_empty() && std::fs::write(&dest, &bytes).is_ok() => n += 1,
            _ => {}
        }
    }
    n
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// Builds a tiny snapshot (1 hero, 1 patch) as a `.tar.gz` file and returns its path.
    fn tiny_snapshot(dir: &Path) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let file = dir.join("referential.tar.gz");
        let gz = flate2::write::GzEncoder::new(
            std::fs::File::create(&file).unwrap(),
            flate2::Compression::fast(),
        );
        let mut tar = tar::Builder::new(gz);
        let mut add = |name: &str, body: &str| {
            let mut h = tar::Header::new_gnu();
            h.set_size(body.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            tar.append_data(&mut h, name, body.as_bytes()).unwrap();
        };
        add(
            "heroes.json",
            r#"[{"name":"Seedtest Hero","role":"Warrior","universe":"Warcraft"}]"#,
        );
        add(
            "patches.json",
            r#"{"items":[{"internalId":"seedtest-1","patchName":"Seed Test Patch","patchType":"Live","liveDate":"2026-01-01T00:00:00Z","heroCount":1,"mapCount":0}]}"#,
        );
        add("images/heroes/seedtest.png", "png");
        tar.into_inner().unwrap().finish().unwrap();
        file
    }

    #[tokio::test]
    async fn ingest_snapshot_file_seeds_heroes_and_patches() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL missing → snapshot seed test skipped");
            return;
        };
        let db = sqlx::postgres::PgPoolOptions::new()
            .connect(&url)
            .await
            .expect("connexion");
        sqlx::migrate!("./migrations")
            .run(&db)
            .await
            .expect("migrations");
        let root = std::env::temp_dir().join(format!("storm-codex-seed-{}", uuid::Uuid::new_v4()));
        let file = tiny_snapshot(&root);
        let images = root.join("images-out");

        let _ = ingest_snapshot_file(&db, &images, &file).await;

        let hero: Option<String> =
            sqlx::query_scalar("SELECT role FROM dim_heroes WHERE id = 'Seedtest Hero'")
                .fetch_optional(&db)
                .await
                .unwrap();
        assert_eq!(hero.as_deref(), Some("Warrior"));
        let patch: Option<String> =
            sqlx::query_scalar("SELECT name FROM dim_patches WHERE internal_id = 'seedtest-1'")
                .fetch_optional(&db)
                .await
                .unwrap();
        assert_eq!(patch.as_deref(), Some("Seed Test Patch"));
        assert!(images.join("heroes/seedtest.png").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn ingest_snapshot_file_missing_file_returns_empty_without_panic() {
        // A lazy pool never connects: the missing file must bail out before any query.
        let db = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://nobody@127.0.0.1:1/none")
            .unwrap();
        let missing = std::env::temp_dir().join(format!("absent-{}.tar.gz", uuid::Uuid::new_v4()));
        let out = ingest_snapshot_file(&db, &std::env::temp_dir(), &missing).await;
        assert!(out.is_empty());
    }
}
