//! Persistence of the draft state (singleton `draft_live`, id=1). Runtime queries (no checked
//! macro), like the rest of the server.
use crate::draft::DraftState;
use sqlx::PgPool;
use tokio::sync::{mpsc, oneshot};

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

/// One `api::persist` request: a snapshot at version `v`, replying on `.2` once handled.
pub type SaveRequest = (DraftState, u64, oneshot::Sender<Result<(), sqlx::Error>>);

/// Spawns the single task that actually writes `draft_live` (same pattern as
/// `cast::store::spawn_saver`). One task, one write in flight at a time, processed in arrival order,
/// and a snapshot whose version is not newer than the last one written is skipped: whatever order
/// concurrent persists (routes, the auto-play timer, the cast sync) reach the channel in, the
/// database ends on the most recent snapshot, and no lock is held across the write's `.await`. The
/// task ends when every `SaveRequest` sender (held by `AppState`) is dropped.
pub fn spawn_saver(db: PgPool) -> mpsc::Sender<SaveRequest> {
    let (tx, mut rx) = mpsc::channel::<SaveRequest>(64);
    tokio::spawn(async move {
        let mut last = 0u64;
        while let Some((snapshot, v, reply)) = rx.recv().await {
            let res = if v <= last {
                Ok(())
            } else {
                let r = save(&db, &snapshot).await;
                if r.is_ok() {
                    last = v;
                }
                r
            };
            let _ = reply.send(res);
        }
    });
    tx
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub(crate) mod test_support {
    use sqlx::PgPool;

    /// A pool whose `draft_live` is a private, empty copy (own schema first in `search_path`), so a
    /// test can read back what it wrote while other tests write the shared singleton row.
    pub(crate) async fn private_draft_live(base: &PgPool) -> (PgPool, String) {
        let schema = format!("draft_race_{}", uuid::Uuid::new_v4().simple());
        for q in [
            format!("CREATE SCHEMA {schema}"),
            format!("CREATE TABLE {schema}.draft_live (LIKE public.draft_live INCLUDING ALL)"),
        ] {
            // test-generated schema name (uuid hex): no external input
            sqlx::query(sqlx::AssertSqlSafe(q))
                .execute(base)
                .await
                .unwrap();
        }
        let opts: sqlx::postgres::PgConnectOptions =
            std::env::var("DATABASE_URL").unwrap().parse().unwrap();
        let db = sqlx::postgres::PgPoolOptions::new()
            .max_connections(16)
            .connect_with(opts.options([("search_path", schema.as_str())]))
            .await
            .unwrap();
        (db, schema)
    }

    pub(crate) async fn drop_private(base: &PgPool, schema: &str) {
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(base)
            .await
            .unwrap();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::draft::{Format, Side};

    fn state(score: [u8; 2]) -> DraftState {
        let mut d = DraftState::new(Format::Standard, Side::Blue, "Sky Temple".into());
        d.score = score;
        d
    }

    async fn send(tx: &mpsc::Sender<SaveRequest>, d: DraftState, v: u64) {
        let (reply, rx) = oneshot::channel();
        tx.send((d, v, reply)).await.unwrap();
        rx.await.unwrap().unwrap();
    }

    /// The saver's own ordering guard: a snapshot that reaches the channel **after** a newer one
    /// (its persist was descheduled between taking its version and sending) is skipped.
    #[tokio::test]
    async fn saver_never_writes_an_older_snapshot_after_a_newer_one() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            return;
        };
        let base = PgPool::connect(&url).await.unwrap();
        sqlx::migrate!("./migrations").run(&base).await.unwrap();
        let (db, schema) = test_support::private_draft_live(&base).await;
        let tx = spawn_saver(db.clone());
        send(&tx, state([1, 0]), 1).await;
        send(&tx, state([3, 0]), 3).await;
        send(&tx, state([2, 0]), 2).await;
        let saved = load(&db).await.unwrap().score;
        test_support::drop_private(&base, &schema).await;
        assert_eq!(
            saved,
            [3, 0],
            "version 2 arrived after version 3 and overwrote it"
        );
    }
}
