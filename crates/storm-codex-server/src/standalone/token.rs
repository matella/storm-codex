//! Upload token of the in-process watcher.
//!
//! `/api/upload` always requires a Bearer whose SHA-256 is in `upload_tokens`; the standalone
//! watcher is no exception. At each start a fresh random token is generated and its hash is
//! written to the single `upload_tokens` row named [`TOKEN_NAME`] (inserted the first time,
//! then updated in place, `revoked_at` cleared). The clear token only lives in memory: it is
//! never logged or written to disk, and the previous session's token stops working. Keeping
//! one row (same id) preserves the `uploads.token_id` history.

use rand::RngCore;

use crate::upload::sha256_hex;

/// `upload_tokens.name` of the standalone watcher's token.
pub const TOKEN_NAME: &str = "standalone-watcher";

/// Serialises concurrent provisioning (two processes on the same database).
const ADVISORY_LOCK_KEY: i64 = 0x5354_4f52_4d57_4154; // "STORMWAT"

/// Rotates (or creates) the token named `name` and returns its clear value.
pub async fn provision(db: &sqlx::PgPool, name: &str) -> Result<String, sqlx::Error> {
    let mut raw = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut raw);
    let token = hex::encode(raw);
    let hash = sha256_hex(token.as_bytes());

    let mut tx = db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(ADVISORY_LOCK_KEY)
        .execute(&mut *tx)
        .await?;
    let updated = sqlx::query(
        "UPDATE upload_tokens SET token_hash = $2, revoked_at = NULL
         WHERE id = (SELECT min(id) FROM upload_tokens WHERE name = $1)",
    )
    .bind(name)
    .bind(&hash)
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() == 0 {
        sqlx::query("INSERT INTO upload_tokens (name, token_hash) VALUES ($1, $2)")
            .bind(name)
            .bind(&hash)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(token)
}
