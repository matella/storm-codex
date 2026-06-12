//! Référentiel héros/talents (`dim_*`) répliqué depuis l'API HotsPatchNotes au démarrage —
//! source unique (pas de second pipeline de sync). Best-effort : une indispo n'empêche pas le
//! serveur de démarrer (le front a un fallback couleur). Config `HOTSPATCHNOTES_URL`.

use sqlx::PgPool;

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
