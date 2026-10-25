//! Historique daté des pseudos. Le pseudo actuel d'un compte est le dernier vu **dans le temps**
//! (`matches.played_at`), pas `players.last_name` (écrasé dans l'ordre de traitement du backfill).

use chrono::{DateTime, Utc};
use sqlx::PgPool;

#[derive(Debug, Clone, PartialEq, serde::Serialize, sqlx::FromRow)]
pub struct NameSpan {
    pub name: String,
    pub first_seen: Option<DateTime<Utc>>,
    pub last_seen: Option<DateTime<Utc>>,
    pub games: i64,
}

/// Triés du plus récent au plus ancien (`last_seen` DESC, NULL en dernier, puis nom ASC : ordre stable).
pub async fn name_history(db: &PgPool, toon: &str) -> Result<Vec<NameSpan>, sqlx::Error> {
    sqlx::query_as(
        "SELECT mp.name AS name, min(m.played_at) AS first_seen, max(m.played_at) AS last_seen, count(*) AS games
         FROM match_players mp JOIN matches m ON m.id = mp.match_id
         WHERE mp.toon_handle = $1 AND mp.name IS NOT NULL
         GROUP BY mp.name
         ORDER BY max(m.played_at) DESC NULLS LAST, mp.name ASC",
    )
    .bind(toon)
    .fetch_all(db)
    .await
}

/// Pseudo actuel = premier de `name_history`, sinon `players.last_name`, sinon `None`.
#[allow(dead_code)] // consommé par la recherche (tâche 2)
pub async fn current_name(db: &PgPool, toon: &str) -> Result<Option<String>, sqlx::Error> {
    if let Some(first) = name_history(db, toon).await?.into_iter().next() {
        return Ok(Some(first.name));
    }
    sqlx::query_scalar("SELECT last_name FROM players WHERE toon_handle = $1")
        .bind(toon)
        .fetch_optional(db)
        .await
        .map(Option::flatten)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::players::testutil::*;
    use chrono::TimeZone;

    #[tokio::test]
    async fn historique_ordonne_et_current_name() {
        let Some(db) = test_pool().await else { return };
        let (toon, u) = (format!("t-{}", uid()), uid());
        let (old, new, undated) = (format!("Old{u}"), format!("New{u}"), format!("Undated{u}"));
        let t = |d| Utc.with_ymd_and_hms(2025, d, 1, 20, 0, 0).unwrap();
        // Ordre d'insertion volontairement faux : le récent d'abord (backfill en désordre).
        insert_match(&db, Some(t(9)), &[row(&toon, &new, 0, "Muradin")]).await;
        insert_match(&db, Some(t(2)), &[row(&toon, &old, 0, "Muradin")]).await;
        insert_match(&db, Some(t(3)), &[row(&toon, &old, 0, "Muradin")]).await;
        insert_match(&db, None, &[row(&toon, &undated, 0, "Muradin")]).await;
        let h = name_history(&db, &toon).await.unwrap();
        let names: Vec<_> = h.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, [new.as_str(), old.as_str(), undated.as_str()]);
        assert_eq!(h[1].games, 2);
        assert_eq!(h[1].first_seen, Some(t(2)));
        assert_eq!(h[1].last_seen, Some(t(3)));
        assert_eq!(h[2].first_seen, None);
        assert_eq!(current_name(&db, &toon).await.unwrap(), Some(new));
    }

    #[tokio::test]
    async fn un_seul_pseudo_un_seul_span_et_inconnu_vide() {
        let Some(db) = test_pool().await else { return };
        let (toon, u) = (format!("t-{}", uid()), uid());
        insert_match(&db, None, &[row(&toon, &format!("Solo{u}"), 0, "Muradin")]).await;
        assert_eq!(name_history(&db, &toon).await.unwrap().len(), 1);
        assert!(name_history(&db, "toon-inexistant").await.unwrap().is_empty());
        assert_eq!(current_name(&db, "toon-inexistant").await.unwrap(), None);
    }
}
