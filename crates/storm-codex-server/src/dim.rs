//! Référentiel héros/talents (`dim_*`) répliqué depuis l'API HotsPatchNotes au démarrage —
//! source unique (pas de second pipeline de sync). Best-effort : une indispo n'empêche pas le
//! serveur de démarrer (le front a un fallback couleur). Config `HOTSPATCHNOTES_URL`.

use sqlx::PgPool;
use std::path::{Path, PathBuf};

/// Réplique les héros depuis `<base>/api/heroes` dans `dim_heroes`. Clé = `name` (storm-stats
/// utilise les noms de héros). Best-effort.
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
            tracing::warn!("dim_heroes : réponse inattendue de HotsPatchNotes");
            return;
        }
        Ok(Err(e)) => {
            tracing::warn!("dim_heroes : HotsPatchNotes indispo ({e}) — anneaux d'univers en fallback");
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
        let _ = sqlx::query(
            "INSERT INTO dim_heroes (id, name, role, universe, data)
             VALUES ($1,$2,$3,$4,$5)
             ON CONFLICT (id) DO UPDATE SET name=EXCLUDED.name, role=EXCLUDED.role,
                universe=EXCLUDED.universe, data=EXCLUDED.data",
        )
        .bind(name) // id = name (clé de jointure avec storm-stats)
        .bind(name)
        .bind(role)
        .bind(universe)
        .bind(h)
        .execute(db)
        .await;
        n += 1;
    }
    tracing::info!("dim_heroes synchronisé : {n} héros depuis HotsPatchNotes");
}

/// Réplique la LISTE des patch notes depuis `<base>/api/patches` dans `dim_patches` (storm-codex en
/// devient propriétaire). Retourne les patches NOUVEAUX (absents avant) → l'appelant les notifie.
/// Aucun « nouveau » au tout premier seed (table vide) pour éviter 200 notifications. Best-effort.
pub async fn sync_patches(db: &PgPool, base_url: &str) -> Vec<(String, String)> {
    let url = format!("{}/api/patches?page=1&pageSize=200", base_url.trim_end_matches('/'));
    let items = match tokio::task::spawn_blocking(move || fetch_json(&url)).await {
        Ok(Ok(v)) => v.get("items").and_then(|i| i.as_array()).cloned().unwrap_or_default(),
        _ => {
            tracing::warn!("dim_patches : HotsPatchNotes indispo");
            return Vec::new();
        }
    };
    let was_empty: i64 = sqlx::query_scalar("SELECT count(*) FROM dim_patches")
        .fetch_one(db).await.unwrap_or(0);
    let mut new_patches: Vec<(String, String)> = Vec::new();
    for it in &items {
        let Some(iid) = it.get("internalId").and_then(|v| v.as_str()) else { continue };
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
    tracing::info!("dim_patches synchronisé : {} patches ({} nouveaux)", items.len(), new_patches.len());
    if was_empty == 0 { Vec::new() } else { new_patches } // seed initial → pas de notif
}

/// Réplique les talents par héros depuis `<base>/api/heroes/{shortName}` dans `dim_talents`.
/// Clé de jointure `tree_id` = `talentTreeId`, qui matche `player.talents[TierNChoice]` écrit par
/// le parser. ~90 requêtes séquentielles (bloquant), lancé en tâche de fond ; refresh complet
/// (DELETE + INSERT en transaction). Best-effort : une indispo n'empêche rien (front a un fallback
/// texte sur l'id brut). `tier` = niveau héros (1,4,7,10,13,16,20), `hero_id` = nom (métadonnée).
pub async fn sync_talents(db: &PgPool, base_url: &str) {
    let base = base_url.trim_end_matches('/').to_string();
    let rows = match tokio::task::spawn_blocking(move || collect_talents(&base)).await {
        Ok(r) if !r.is_empty() => r,
        Ok(_) => {
            tracing::warn!("dim_talents : aucun talent collecté (HotsPatchNotes indispo ?)");
            return;
        }
        Err(_) => return, // join error
    };

    let mut tx = match db.begin().await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("dim_talents : begin échoué ({e})");
            return;
        }
    };
    if sqlx::query("DELETE FROM dim_talents").execute(&mut *tx).await.is_err() {
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
        tracing::info!("dim_talents synchronisé : {n} talents depuis HotsPatchNotes");
    }
}

/// Collecte bloquante : liste héros → détail par `shortName` → talents aplatis.
/// Tuple = (hero_id, tier=niveau, name, tree_id=talentTreeId, data{icon,type,description}).
/// `hero_id` = **nom canonique du parser** (via `attributeId` HotsPatchNotes → `attr.json` de
/// storm-stats), pas le nom HotsPatchNotes : ils diffèrent en ponctuation/articles (« Li-Ming » vs
/// « LiMing », « The Lost Vikings » vs « LostVikings »). L'aligner sur le parser évite de fausses
/// corrections lors de la résolution du héros joué (cf. project.rs::talent_hero).
fn collect_talents(base: &str) -> Vec<(String, i32, String, String, serde_json::Value)> {
    let mut out = Vec::new();
    // code attribut (4 lettres) → nom canonique parser (table figée de hots-parser).
    let code_to_name: std::collections::HashMap<String, String> =
        storm_stats::constants::hero_attribute()
            .as_object()
            .map(|o| {
                o.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();
    let Ok(list) = fetch_array(&format!("{base}/api/heroes")) else { return out };
    for h in &list {
        let Some(short) = h.get("shortName").and_then(|v| v.as_str()) else { continue };
        let Some(hp_name) = h.get("name").and_then(|v| v.as_str()) else { continue };
        let Ok(detail) = fetch_json(&format!("{base}/api/heroes/{short}")) else { continue };
        // attributeId HotsPatchNotes == code attr.json (ex. Johanna="Crus", E.T.C.="L90E") →
        // nom canonique parser ; fallback sur le nom HotsPatchNotes si absent/non mappé.
        let hero_name = detail
            .get("attributeId")
            .and_then(|v| v.as_str())
            .and_then(|a| code_to_name.get(a))
            .map(String::as_str)
            .unwrap_or(hp_name);
        let Some(talents) = detail.get("talents").and_then(|v| v.as_object()) else { continue };
        for (level_key, arr) in talents {
            let tier: i32 = level_key.parse().unwrap_or(0);
            let Some(arr) = arr.as_array() else { continue };
            for t in arr {
                let Some(tree_id) = t.get("talentTreeId").and_then(|v| v.as_str()) else { continue };
                let name = t.get("name").and_then(|v| v.as_str()).unwrap_or(tree_id);
                let data = serde_json::json!({
                    "icon": t.get("icon"),
                    "type": t.get("type"),
                    "description": t.get("description"),
                    "sort": t.get("sort"),
                    "isQuest": t.get("isQuest"),
                });
                out.push((hero_name.to_string(), tier, name.to_string(), tree_id.to_string(), data));
            }
        }
    }
    out
}

/// Vendorise (une fois) les portraits héros + images de cartes depuis HotsPatchNotes dans
/// `images_dir` (servi sur `/images`). Idempotent : saute les fichiers déjà présents. Best-effort
/// — une indispo n'empêche rien (le front a un fallback). Rend storm-codex auto-suffisant.
pub async fn vendor_images(images_dir: &Path, base_url: &str) {
    let base = base_url.trim_end_matches('/').to_string();
    let dir = images_dir.to_path_buf();
    let _ = tokio::task::spawn_blocking(move || {
        let mut got = 0;
        // /api/heroes → champ "icon" ("/images/heroes/<slug>.png")
        if let Ok(list) = fetch_array(&format!("{base}/api/heroes")) {
            got += download_referenced(&list, "icon", &base, &dir);
        }
        // /api/battlegrounds → champ "imageUrl" ("/images/battlegrounds/<slug>.png")
        if let Ok(list) = fetch_array(&format!("{base}/api/battlegrounds")) {
            got += download_referenced(&list, "imageUrl", &base, &dir);
        }
        tracing::info!("images vendorisées : {got} nouveaux fichiers dans {dir:?}");
    })
    .await;
}

/// Ingestion du **snapshot référentiel** (mode bundle autonome) : télécharge `referential.tar.gz`
/// (`REFERENTIAL_URL`), le décompresse, et peuple `dim_heroes` / `dim_talents` / `dim_patches`
/// (+ détail patch fusionné dans `data.detail` pour lecture offline) puis vendorise les images dans
/// `images_dir`. Aucune dépendance runtime à HotsPatchNotes. Retourne les patches NOUVEAUX (pour
/// notif), comme `sync_patches` ; aucun « nouveau » au premier seed (table vide). Best-effort.
pub async fn ingest_snapshot(db: &PgPool, images_dir: &Path, url: &str) -> Vec<(String, String)> {
    let url = url.to_string();
    let dir = images_dir.to_path_buf();
    let tmp = std::env::temp_dir().join("storm-codex-referential");

    // Téléchargement + décompression (bloquant : réseau + tar/gzip).
    let prep = tokio::task::spawn_blocking({
        let tmp = tmp.clone();
        move || -> Result<(), String> {
            let bytes = ureq::get(&url)
                .call()
                .map_err(|e| e.to_string())?
                .body_mut()
                .read_to_vec()
                .map_err(|e| e.to_string())?;
            let _ = std::fs::remove_dir_all(&tmp);
            std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
            let gz = flate2::read::GzDecoder::new(&bytes[..]);
            tar::Archive::new(gz).unpack(&tmp).map_err(|e| e.to_string())?;
            Ok(())
        }
    })
    .await;
    match prep {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            tracing::warn!("référentiel : snapshot indispo ({e}) — anneaux d'univers en fallback");
            return Vec::new();
        }
        Err(_) => return Vec::new(),
    }

    let read = |name: &str| -> Option<serde_json::Value> {
        std::fs::read_to_string(tmp.join(name))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
    };

    // dim_heroes (même schéma que sync_heroes : id = name).
    if let Some(serde_json::Value::Array(heroes)) = read("heroes.json") {
        let mut n = 0;
        for h in &heroes {
            let Some(name) = h.get("name").and_then(|v| v.as_str()) else { continue };
            let _ = sqlx::query(
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
            .await;
            n += 1;
        }
        tracing::info!("dim_heroes (snapshot) : {n} héros");
    }

    // dim_talents depuis hero-details.json ({shortName: détail}). Même mapping canonique que
    // collect_talents : attributeId HotsPatchNotes → nom parser (attr.json) ; fallback nom HPN.
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
            if sqlx::query("DELETE FROM dim_talents").execute(&mut *tx).await.is_ok() {
                let mut n = 0;
                for detail in details.values() {
                    let hp_name = detail.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let hero_name = detail
                        .get("attributeId")
                        .and_then(|v| v.as_str())
                        .and_then(|a| code_to_name.get(a))
                        .map(String::as_str)
                        .unwrap_or(hp_name);
                    let Some(talents) = detail.get("talents").and_then(|v| v.as_object()) else { continue };
                    for (level_key, arr) in talents {
                        let tier: i32 = level_key.parse().unwrap_or(0);
                        let Some(arr) = arr.as_array() else { continue };
                        for t in arr {
                            let Some(tree_id) = t.get("talentTreeId").and_then(|v| v.as_str()) else { continue };
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
                    tracing::info!("dim_talents (snapshot) : {n} talents");
                }
            }
        }
    }

    // dim_patches : items + détail fusionné (data.detail) pour le rendu offline du patch.
    let mut new_patches: Vec<(String, String)> = Vec::new();
    if let Some(items) = read("patches.json")
        .and_then(|v| v.get("items").and_then(|i| i.as_array()).cloned())
    {
        let pdetails = read("patch-details.json").unwrap_or(serde_json::Value::Null);
        let was_empty: i64 = sqlx::query_scalar("SELECT count(*) FROM dim_patches")
            .fetch_one(db)
            .await
            .unwrap_or(0);
        for it in &items {
            let Some(iid) = it.get("internalId").and_then(|v| v.as_str()) else { continue };
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
        }
        tracing::info!("dim_patches (snapshot) : {} patches ({} nouveaux)", items.len(), new_patches.len());
        if was_empty == 0 {
            new_patches.clear(); // seed initial → pas de notif
        }
    }

    // Images : copie images/** du snapshot vers images_dir (idempotent, saute l'existant).
    let img_src = tmp.join("images");
    let _ = tokio::task::spawn_blocking(move || {
        let got = copy_tree(&img_src, &dir);
        let _ = std::fs::remove_dir_all(&tmp);
        tracing::info!("images (snapshot) : {got} fichiers copiés vers {dir:?}");
    })
    .await;

    new_patches
}

/// Copie récursive `src` → `dst` (crée les dossiers, saute les fichiers déjà présents). Retourne le
/// nombre de fichiers écrits. Best-effort (les erreurs par fichier sont ignorées).
fn copy_tree(src: &Path, dst: &Path) -> u32 {
    let mut n = 0;
    let Ok(entries) = std::fs::read_dir(src) else { return 0 };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name() else { continue };
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

fn fetch_array(url: &str) -> Result<Vec<serde_json::Value>, String> {
    let body = ureq::get(url)
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    match serde_json::from_str(&body) {
        Ok(serde_json::Value::Array(a)) => Ok(a),
        _ => Err("réponse non-array".into()),
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

/// Pour chaque objet, lit `field` (chemin `/images/...`), télécharge `<base><path>` vers
/// `<images_dir><path sans /images>` si absent. Retourne le nombre de fichiers téléchargés.
fn download_referenced(list: &[serde_json::Value], field: &str, base: &str, images_dir: &Path) -> u32 {
    let mut n = 0;
    for obj in list {
        let Some(path) = obj.get(field).and_then(|v| v.as_str()) else { continue };
        // attendu : "/images/heroes/x.png" ou "/images/battlegrounds/x.png"
        let Some(rel) = path.strip_prefix("/images/") else { continue };
        let dest: PathBuf = images_dir.join(rel);
        if dest.exists() {
            continue;
        }
        if let Some(parent) = dest.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let url = format!("{base}{path}");
        match ureq::get(&url).call().and_then(|mut r| r.body_mut().read_to_vec()) {
            Ok(bytes) if !bytes.is_empty() => {
                if std::fs::write(&dest, &bytes).is_ok() {
                    n += 1;
                }
            }
            _ => {} // 404 (ex. carte ARAM sans image) → on saute, le front a un fallback
        }
    }
    n
}
