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

/// `schema_version` persisté plus récent que celui du code (état écrit par une version plus neuve
/// du serveur) : chargé quand même, de façon tolérante, mais signalé.
pub(super) fn newer_schema(v: &serde_json::Value) -> Option<i64> {
    v.get("schema_version")
        .and_then(serde_json::Value::as_i64)
        .filter(|n| *n > i64::from(super::SCHEMA_VERSION))
}

/// Charge la série persistée, ou `None` si absente/illisible (→ état neuf au démarrage). Une erreur
/// de lecture en base est journalisée en `error` ; un JSON illisible en `warn` (cas distincts).
pub async fn load(db: &PgPool) -> Option<Series> {
    let row: Option<serde_json::Value> =
        match sqlx::query_scalar("SELECT state FROM cast_live WHERE id = 1").fetch_optional(db).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("cast load: lecture de cast_live impossible ({e}) — série neuve");
                return None;
            }
        };
    let v = row?;
    if let Some(n) = newer_schema(&v) {
        tracing::warn!("cast load: schema_version {n} plus récent que le code ({}) — lecture tolérante", super::SCHEMA_VERSION);
    }
    let s = decode(v);
    if s.is_none() {
        tracing::warn!("cast load: état cast_live illisible — série neuve");
    }
    s
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
