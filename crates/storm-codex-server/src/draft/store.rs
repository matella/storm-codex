//! Persistance de l'état de draft (singleton `draft_live`, id=1). Requêtes runtime (pas de macro
//! vérifiée), comme le reste du serveur.
use crate::draft::DraftState;
use sqlx::PgPool;

/// Charge l'état persistant, ou `None` si absent/illisible (→ état neuf au démarrage).
pub async fn load(db: &PgPool) -> Option<DraftState> {
    let row: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT state FROM draft_live WHERE id = 1")
            .fetch_optional(db)
            .await
            .ok()
            .flatten();
    row.and_then(|j| serde_json::from_value(j).ok())
}

/// Écrit l'état (upsert sur la ligne unique).
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
