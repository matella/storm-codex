//! Persistence of the current lobby (singleton `lobby_live`, id=1). Runtime queries, like the rest
//! of the server. Deliberate copy of the pattern of `draft/store.rs`: two independent singletons, each
//! with its own life cycle; factoring them would cost more in indirection than it would bring.
use crate::lobby::{LobbyState, LOBBY_SCHEMA_VERSION};
use sqlx::PgPool;

/// Loads the persisted state. A state of another schema version is ignored (→ no lobby).
pub async fn load(db: &PgPool) -> Option<LobbyState> {
    let row: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT state FROM lobby_live WHERE id = 1")
            .fetch_optional(db)
            .await
            .ok()
            .flatten();
    let state: LobbyState = row.and_then(|j| serde_json::from_value(j).ok())?;
    (state.schema_version == LOBBY_SCHEMA_VERSION).then_some(state)
}

/// Writes the state (upsert on the single row).
pub async fn save(db: &PgPool, state: &LobbyState) -> Result<(), sqlx::Error> {
    let v = serde_json::to_value(state).unwrap_or_else(|_| serde_json::json!({}));
    sqlx::query(
        "INSERT INTO lobby_live (id, state, updated_at) VALUES (1, $1, now())
         ON CONFLICT (id) DO UPDATE SET state = EXCLUDED.state, updated_at = now()",
    )
    .bind(&v)
    .execute(db)
    .await
    .map(|_| ())
}

/// Clears the current lobby.
pub async fn clear(db: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM lobby_live WHERE id = 1")
        .execute(db)
        .await
        .map(|_| ())
}
