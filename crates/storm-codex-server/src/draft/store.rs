//! Persistence of the draft state (singleton `draft_live`, id=1). Runtime queries (no checked
//! macro), like the rest of the server.
use crate::draft::DraftState;
use sqlx::PgPool;

/// Loads the persisted state, or `None` if missing/unreadable (→ fresh state at startup).
pub async fn load(db: &PgPool) -> Option<DraftState> {
    let row: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT state FROM draft_live WHERE id = 1")
            .fetch_optional(db)
            .await
            .ok()
            .flatten();
    row.and_then(|j| serde_json::from_value(j).ok())
}

/// Writes the state (upsert on the single row).
pub async fn save(db: &PgPool, state: &DraftState) -> Result<(), sqlx::Error> {
    let v = serde_json::to_value(state).unwrap_or_else(|_| serde_json::json!({}));
    sqlx::query(
        "INSERT INTO draft_live (id, state, updated_at) VALUES (1, $1, now())
         ON CONFLICT (id) DO UPDATE SET state = EXCLUDED.state, updated_at = now()",
    )
    .bind(&v)
    .execute(db)
    .await
    .map(|_| ())
}
