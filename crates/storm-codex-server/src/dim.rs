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
