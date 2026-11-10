//! Persistence of the cast series (singleton `cast_live`, id=1). Runtime queries, like `draft::store`.
use super::Series;
use sqlx::PgPool;

/// Decodes a persisted state. `None` on any error (non-object, `players` array of the wrong
/// length, unknown enum value…): startup must never fail.
pub(super) fn decode(v: serde_json::Value) -> Option<Series> {
    if !v.is_object() {
        return None;
    }
    serde_json::from_value(v).ok()
}

/// Persisted `schema_version` more recent than the code's (state written by a newer version
/// of the server): loaded anyway, leniently, but reported.
pub(super) fn newer_schema(v: &serde_json::Value) -> Option<i64> {
    v.get("schema_version")
        .and_then(serde_json::Value::as_i64)
        .filter(|n| *n > i64::from(super::SCHEMA_VERSION))
}

/// Loads the persisted series, or `None` if missing/unreadable (→ fresh state at startup). A database
/// read error is logged as `error`; an unreadable JSON as `warn` (distinct cases).
pub async fn load(db: &PgPool) -> Option<Series> {
    let row: Option<serde_json::Value> =
        match sqlx::query_scalar("SELECT state FROM cast_live WHERE id = 1")
            .fetch_optional(db)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("cast load: cannot read cast_live ({e}) — fresh series");
                return None;
            }
        };
    let v = row?;
    if let Some(n) = newer_schema(&v) {
        tracing::warn!(
            "cast load: schema_version {n} newer than the code ({}) — lenient read",
            super::SCHEMA_VERSION
        );
    }
    let s = decode(v);
    if s.is_none() {
        tracing::warn!("cast load: unreadable cast_live state — fresh series");
    }
    s
}

/// Writes the series (upsert on the single row).
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
