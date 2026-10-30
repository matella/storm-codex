//! Héros récents par joueur pour les cartes d'équipe de la régie : fenêtre des 20 dernières
//! parties (tous modes) du toon, 3 héros les plus joués (parties puis récence). Moins de
//! `MIN_GAMES` parties dans la fenêtre → liste vide (masquée, jamais partielle).
use serde::Serialize;
use sqlx::PgPool;

pub const WINDOW: i64 = 20;
pub const MIN_GAMES: usize = 5;
pub const TOP: usize = 3;
/// Nombre maximal de toons par équipe acceptés par la route.
pub const MAX_PER_SIDE: usize = 10;

#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct HeroCount {
    pub hero: String,
    pub games: u32,
}

/// Une seule requête pour tous les toons (LATERAL sur `match_players_toon_idx`) ; la sortie garde
/// la position : `out[i]` correspond à `toons[i]` (toon vide = joueur non lié → liste vide).
pub async fn recent_heroes_many(db: &PgPool, toons: &[String]) -> Result<Vec<Vec<HeroCount>>, sqlx::Error> {
    let rows: Vec<(i32, Option<String>)> = sqlx::query_as(LINEUP_SQL).bind(toons).bind(WINDOW).fetch_all(db).await?;
    Ok(tally(toons.len(), &rows))
}

/// API à un seul toon, pour les tests (la route passe par `recent_heroes_many`).
#[cfg(test)]
pub async fn recent_heroes(db: &PgPool, toon: &str) -> Result<Vec<HeroCount>, sqlx::Error> {
    Ok(recent_heroes_many(db, &[toon.to_owned()]).await?.into_iter().next().unwrap_or_default())
}

pub(crate) const LINEUP_SQL: &str = "
SELECT t.ord::int4, w.hero
FROM unnest($1::text[]) WITH ORDINALITY AS t(toon, ord)
CROSS JOIN LATERAL (
    SELECT mp.hero, m.played_at, m.id
    FROM match_players mp JOIN matches m ON m.id = mp.match_id
    WHERE mp.toon_handle = t.toon
    ORDER BY m.played_at DESC NULLS LAST, m.id DESC
    LIMIT $2
) w
WHERE t.toon <> ''
ORDER BY t.ord, w.played_at DESC NULLS LAST, w.id DESC";

/// `rows` : `(position 1-based, héros)` triés par position puis récence décroissante. Une ligne au
/// héros NULL compte dans le seuil `MIN_GAMES` (c'est une partie jouée) mais pas dans le décompte.
fn tally(n: usize, rows: &[(i32, Option<String>)]) -> Vec<Vec<HeroCount>> {
    let mut games = vec![0usize; n];
    // par position : (héros, parties, rang de la 1re apparition = récence)
    let mut per: Vec<Vec<(String, u32, usize)>> = vec![Vec::new(); n];
    for (ord, hero) in rows {
        let Some(i) = usize::try_from(*ord).ok().and_then(|o| o.checked_sub(1)).filter(|i| *i < n) else { continue };
        games[i] += 1;
        let Some(h) = hero else { continue };
        match per[i].iter_mut().find(|(x, _, _)| x == h) {
            Some(e) => e.1 += 1,
            None => per[i].push((h.clone(), 1, games[i])),
        }
    }
    per.into_iter()
        .zip(games)
        .map(|(mut v, g)| {
            if g < MIN_GAMES {
                return Vec::new();
            }
            v.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)));
            v.into_iter().take(TOP).map(|(hero, games, _)| HeroCount { hero, games }).collect()
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::api_tests::{call, test_state};
    use crate::players::testutil::{insert_match, row, test_pool, uid};
    use chrono::{Duration, TimeZone, Utc};
    use std::time::{Duration as StdDuration, Instant};

    /// `heroes[i]` joué à la partie i (la plus ancienne d'abord), une partie par jour.
    async fn seed(db: &PgPool, toon: &str, heroes: &[&str]) {
        let t0 = Utc.with_ymd_and_hms(2025, 1, 1, 20, 0, 0).unwrap();
        for (i, h) in heroes.iter().enumerate() {
            insert_match(db, Some(t0 + Duration::days(i as i64)), &[row(toon, "Casteur", 0, h)]).await;
        }
    }
    fn rep(h: &'static str, n: usize) -> Vec<&'static str> {
        vec![h; n]
    }
    fn hc(hero: &str, games: u32) -> HeroCount {
        HeroCount { hero: hero.into(), games }
    }

    #[tokio::test]
    async fn recent_heroes_top3_over_last_20_games() {
        let Some(db) = test_pool().await else { return };
        let toon = format!("t-lu{}", uid());
        // 5 plus anciennes : Muradin (hors fenêtre) ; 20 récentes : 10 Kerrigan, 6 Diablo, 3 Arthas, 1 Zeratul
        let mut h = rep("Muradin", 5);
        h.extend(rep("Kerrigan", 10));
        h.extend(rep("Diablo", 6));
        h.extend(rep("Arthas", 3));
        h.push("Zeratul");
        seed(&db, &toon, &h).await;
        let got = recent_heroes(&db, &toon).await.unwrap();
        assert_eq!(got, vec![hc("Kerrigan", 10), hc("Diablo", 6), hc("Arthas", 3)]);
    }

    #[tokio::test]
    async fn recent_heroes_tie_broken_by_recency() {
        let Some(db) = test_pool().await else { return };
        let toon = format!("t-lu{}", uid());
        let mut h = rep("Diablo", 3);
        h.extend(rep("Arthas", 3));
        seed(&db, &toon, &h).await;
        let got = recent_heroes(&db, &toon).await.unwrap();
        assert_eq!(got, vec![hc("Arthas", 3), hc("Diablo", 3)]);
    }

    #[tokio::test]
    async fn recent_heroes_hidden_under_5_games() {
        let Some(db) = test_pool().await else { return };
        let toon = format!("t-lu{}", uid());
        seed(&db, &toon, &rep("Kerrigan", 4)).await;
        assert!(recent_heroes(&db, &toon).await.unwrap().is_empty());
        seed(&db, &toon, &["Kerrigan"]).await;
        assert_eq!(recent_heroes(&db, &toon).await.unwrap(), vec![hc("Kerrigan", 5)]);
    }

    #[tokio::test]
    async fn recent_heroes_counts_all_modes() {
        let Some(db) = test_pool().await else { return };
        let toon = format!("t-lu{}", uid());
        let t0 = Utc.with_ymd_and_hms(2025, 1, 1, 20, 0, 0).unwrap();
        for (i, mode) in [1, 2, 3, 4, 5, 6].into_iter().enumerate() {
            let id = insert_match(&db, Some(t0 + Duration::days(i as i64)), &[row(&toon, "Casteur", 0, "Valla")]).await;
            sqlx::query("UPDATE matches SET mode = $1 WHERE id = $2").bind(mode).bind(id).execute(&db).await.unwrap();
        }
        assert_eq!(recent_heroes(&db, &toon).await.unwrap(), vec![hc("Valla", 6)]);
    }

    #[tokio::test]
    async fn lineup_keeps_position_and_empty_toon_is_empty() {
        let Some(db) = test_pool().await else { return };
        let (a, b) = (format!("t-lu{}", uid()), format!("t-lu{}", uid()));
        seed(&db, &a, &rep("Valla", 6)).await;
        seed(&db, &b, &rep("Jaina", 7)).await;
        let got = recent_heroes_many(&db, &[b.clone(), String::new(), a.clone(), "inconnu".into()]).await.unwrap();
        assert_eq!(got, vec![vec![hc("Jaina", 7)], vec![], vec![hc("Valla", 6)], vec![]]);
    }

    /// Nettoyage des lignes insérées par le test, même s'il panique : `Drop` ne peut pas `.await`,
    /// on supprime donc depuis un thread dédié (runtime et connexion propres).
    struct Cleanup {
        ids: Vec<i64>,
        toons: Vec<String>,
    }
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let (ids, toons) = (std::mem::take(&mut self.ids), std::mem::take(&mut self.toons));
            let Ok(url) = std::env::var("DATABASE_URL") else { return };
            let _ = std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
                rt.block_on(async {
                    let db = PgPool::connect(&url).await.expect("connexion");
                    sqlx::query("DELETE FROM matches WHERE id = ANY($1)").bind(&ids).execute(&db).await.expect("delete matches");
                    sqlx::query("DELETE FROM players WHERE toon_handle = ANY($1)").bind(&toons).execute(&db).await.expect("delete players");
                });
            })
            .join();
        }
    }

    #[tokio::test]
    async fn lineup_route_under_100ms_for_10_players() {
        let Some(state) = test_state(None).await else { return };
        let toons: Vec<String> = (0..10).map(|_| format!("t-lu{}", uid())).collect();
        let mut cleanup = Cleanup { ids: Vec::new(), toons: toons.clone() };
        let t0 = Utc.with_ymd_and_hms(2025, 1, 1, 20, 0, 0).unwrap();
        let heroes = ["Valla", "Jaina", "Diablo", "Arthas", "Zeratul"];
        for g in 0..30 {
            let rows: Vec<_> = toons.iter().enumerate().map(|(i, t)| row(t, "Casteur", (i % 2) as i32, heroes[(g + i) % 5])).collect();
            cleanup.ids.push(insert_match(&state.db, Some(t0 + Duration::days(g as i64)), &rows).await);
        }
        // EXPLAIN : le plan doit passer par l'index `match_players_toon_idx` (scan séquentiel désactivé
        // sur cette connexion : le test ne dépend pas de la taille de la base de dev/CI)
        let mut conn = state.db.acquire().await.unwrap();
        sqlx::query("SET enable_seqscan = off").execute(&mut *conn).await.unwrap();
        let plan: Vec<(String,)> = sqlx::query_as(&format!("EXPLAIN {LINEUP_SQL}"))
            .bind(&toons)
            .bind(WINDOW)
            .fetch_all(&mut *conn)
            .await
            .unwrap();
        sqlx::query("RESET enable_seqscan").execute(&mut *conn).await.unwrap();
        drop(conn);
        let plan = plan.into_iter().map(|r| r.0).collect::<Vec<_>>().join("\n");
        eprintln!("{plan}");
        assert!(plan.contains("match_players_toon_idx"), "plan sans match_players_toon_idx :\n{plan}");

        let q = format!("/api/cast/lineup?blue={}&red={}", toons[..5].join(","), toons[5..].join(","));
        let t = Instant::now();
        let (st, v) = call(&state, "GET", &q, None).await;
        let elapsed = t.elapsed();
        assert_eq!(st, axum::http::StatusCode::OK);
        assert_eq!(v["blue"].as_array().unwrap().len(), 5);
        assert_eq!(v["red"][0].as_array().unwrap().len(), 3);
        assert_eq!(v["blue"][0][0]["games"], 4); // 20 parties, 5 héros en rotation
        assert!(elapsed < StdDuration::from_millis(100), "{elapsed:?}");
        // nettoyage des lignes insérées : `cleanup` (Drop), aussi en cas de panique
    }

    /// Une partie au héros NULL compte dans le seuil des 5 parties mais pas dans le décompte.
    #[test]
    fn null_hero_counts_toward_threshold_not_tally() {
        let rows: Vec<(i32, Option<String>)> =
            vec![(1, Some("Valla".into())), (1, None), (1, None), (1, Some("Valla".into())), (1, None)];
        assert_eq!(tally(1, &rows), vec![vec![hc("Valla", 2)]]);
        assert_eq!(tally(1, &rows[..4]), vec![Vec::<HeroCount>::new()]);
    }
}
