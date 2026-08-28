//! Persistance du lobby courant (singleton `lobby_live`, id=1). Requêtes runtime, comme le reste
//! du serveur. Copie assumée du motif de `draft/store.rs` : deux singletons indépendants, chacun
//! avec son cycle de vie ; les factoriser coûterait plus en indirection qu'il ne rapporterait.
use crate::lobby::{LobbyState, LOBBY_SCHEMA_VERSION};
use sqlx::PgPool;

/// Charge l'état persistant. Un état d'une autre version de schéma est ignoré (→ pas de lobby).
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

/// Écrit l'état (upsert sur la ligne unique).
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

/// Efface le lobby courant.
pub async fn clear(db: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM lobby_live WHERE id = 1")
        .execute(db)
        .await
        .map(|_| ())
}
