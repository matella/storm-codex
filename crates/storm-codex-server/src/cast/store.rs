//! Persistance de la série de cast (singleton `cast_live`, id=1). Requêtes runtime, comme `draft::store`.
use super::Series;
use sqlx::PgPool;

/// Décode un état persisté. `None` sur toute erreur (non-objet, tableau `players` de mauvaise
/// longueur, valeur d'enum inconnue…) : le démarrage ne doit jamais échouer.
pub(super) fn decode(v: serde_json::Value) -> Option<Series> {
    if !v.is_object() {
        return None;
    }
    serde_json::from_value(v).ok()
}

/// Charge la série persistée, ou `None` si absente/illisible (→ état neuf au démarrage).
pub async fn load(db: &PgPool) -> Option<Series> {
    let row: Option<serde_json::Value> = sqlx::query_scalar("SELECT state FROM cast_live WHERE id = 1")
        .fetch_optional(db)
        .await
        .ok()
        .flatten();
    row.and_then(decode)
}

/// Écrit la série (upsert sur la ligne unique).
pub async fn save(db: &PgPool, series: &Series) -> Result<(), sqlx::Error> {
    let v = serde_json::to_value(series).unwrap_or_else(|_| serde_json::json!({}));
    sqlx::query(
        "INSERT INTO cast_live (id, state, updated_at) VALUES (1, $1, now())
         ON CONFLICT (id) DO UPDATE SET state = EXCLUDED.state, updated_at = now()",
    )
    .bind(&v)
    .execute(db)
    .await
    .map(|_| ())
}
