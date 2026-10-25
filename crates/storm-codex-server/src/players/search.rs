//! Recherche de joueurs par pseudo actuel, ancien pseudo ou fragment (`lower()` + `ILIKE`, sans
//! extension Postgres). Tri : exact > préfixe > sous-chaîne, puis nombre de parties.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

pub const MIN_QUERY_CHARS: usize = 2;
pub const DEFAULT_LIMIT: i64 = 20;
pub const MAX_LIMIT: i64 = 50;

#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchHit {
    pub toon: String,
    /// Pseudo actuel (historique daté).
    pub name: Option<String>,
    /// Pseudo qui a matché la requête.
    pub matched_name: String,
    /// `true` si `matched_name` n'est plus le pseudo actuel (« was … »).
    pub matched_old: bool,
    pub games: i64,
    pub wins: i64,
    /// Deux rôles les plus joués `(rôle, parties)`, via `dim_heroes` (vide si référentiel absent).
    pub roles: Vec<(String, i64)>,
    pub last_seen: Option<DateTime<Utc>>,
}

/// Échappe `\`, `%`, `_` pour un motif `LIKE ... ESCAPE '\'` : `Kerrigan_Main` se cherche littéralement.
pub fn escape_like(q: &str) -> String {
    let mut out = String::with_capacity(q.len());
    for c in q.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Requête nettoyée (trim, minuscules), ou `None` si trop courte pour être cherchée.
pub fn normalize_query(q: &str) -> Option<String> {
    let q = q.trim().to_lowercase();
    (q.chars().count() >= MIN_QUERY_CHARS).then_some(q)
}

#[derive(sqlx::FromRow)]
struct HitRow {
    toon_handle: String,
    matched: String,
    games: i64,
    wins: i64,
    current_name: Option<String>,
    last_seen: Option<DateTime<Utc>>,
    roles: sqlx::types::Json<Vec<(String, i64)>>,
}

/// Les agrégats coûteux (victoires, pseudo actuel, rôles) ne sont calculés que pour les `limit`
/// lignes retenues ; seul le nombre de parties (tri) est compté pour tous les candidats.
const SEARCH_SQL: &str = "
WITH cand AS (
    SELECT p.toon_handle,
           (SELECT n FROM jsonb_array_elements_text(p.names) AS n
             WHERE lower(n) LIKE $1 ESCAPE '\\'
             ORDER BY CASE WHEN lower(n) = $2 THEN 0 WHEN lower(n) LIKE $3 ESCAPE '\\' THEN 1 ELSE 2 END, n
             LIMIT 1) AS matched
    FROM players p
    WHERE EXISTS (SELECT 1 FROM jsonb_array_elements_text(p.names) AS n WHERE lower(n) LIKE $1 ESCAPE '\\')
), ranked AS (
    SELECT c.toon_handle, c.matched,
           CASE WHEN lower(c.matched) = $2 THEN 0 WHEN lower(c.matched) LIKE $3 ESCAPE '\\' THEN 1 ELSE 2 END AS tier,
           (SELECT count(*) FROM match_players mp WHERE mp.toon_handle = c.toon_handle) AS games
    FROM cand c ORDER BY tier, games DESC, c.toon_handle LIMIT $4
)
SELECT r.toon_handle, r.matched, r.games,
       (SELECT count(*) FROM match_players mp WHERE mp.toon_handle = r.toon_handle AND mp.win) AS wins,
       (SELECT mp.name FROM match_players mp JOIN matches m ON m.id = mp.match_id
         WHERE mp.toon_handle = r.toon_handle AND mp.name IS NOT NULL
         ORDER BY m.played_at DESC NULLS LAST, mp.name LIMIT 1) AS current_name,
       (SELECT max(m.played_at) FROM match_players mp JOIN matches m ON m.id = mp.match_id
         WHERE mp.toon_handle = r.toon_handle) AS last_seen,
       (SELECT COALESCE(jsonb_agg(jsonb_build_array(x.role, x.g) ORDER BY x.g DESC), '[]'::jsonb) FROM (
            SELECT d.role, count(*) AS g FROM match_players mp JOIN dim_heroes d ON d.name = mp.hero
            WHERE mp.toon_handle = r.toon_handle AND d.role IS NOT NULL
            GROUP BY d.role ORDER BY count(*) DESC, d.role LIMIT 2) x) AS roles
FROM ranked r ORDER BY r.tier, r.games DESC, r.toon_handle";

pub async fn search(db: &PgPool, q: &str, limit: i64) -> Result<Vec<SearchHit>, sqlx::Error> {
    let Some(q) = normalize_query(q) else { return Ok(vec![]) };
    let esc = escape_like(&q);
    let rows: Vec<HitRow> = sqlx::query_as(SEARCH_SQL)
        .bind(format!("%{esc}%"))
        .bind(&q)
        .bind(format!("{esc}%"))
        .bind(limit.clamp(1, MAX_LIMIT))
        .fetch_all(db)
        .await?;
    Ok(rows
        .into_iter()
        .map(|r| SearchHit {
            matched_old: r.current_name.as_deref() != Some(r.matched.as_str()),
            toon: r.toon_handle,
            name: r.current_name,
            matched_name: r.matched,
            games: r.games,
            wins: r.wins,
            roles: r.roles.0,
            last_seen: r.last_seen,
        })
        .collect())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::players::testutil::*;
    use chrono::TimeZone;

    #[test]
    fn escape_like_rend_les_jokers_litteraux() {
        assert_eq!(escape_like("Kerrigan_Main"), "Kerrigan\\_Main");
        assert_eq!(escape_like("100%"), "100\\%");
        assert_eq!(escape_like("a\\b"), "a\\\\b");
        assert_eq!(escape_like("plain"), "plain");
    }

    #[test]
    fn requete_trop_courte_ou_vide_nest_pas_cherchee() {
        assert_eq!(normalize_query(""), None);
        assert_eq!(normalize_query("   "), None);
        assert_eq!(normalize_query(" a "), None);
        assert_eq!(normalize_query("  AbC "), Some("abc".into()));
    }

    fn t(m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2025, m, 1, 20, 0, 0).unwrap()
    }

    #[tokio::test]
    async fn trouve_ancien_pseudo_et_traite_underscore_litteralement() {
        let Some(db) = test_pool().await else { return };
        let u = uid();
        let (a, b, c) = (format!("ta-{u}"), format!("tb-{u}"), format!("tc-{u}"));
        // A : ancien « Zed{u}Night », actuel « Zed{u}Knight ».
        insert_match(&db, Some(t(1)), &[row(&a, &format!("Zed{u}Night"), 0, "Muradin")]).await;
        insert_match(&db, Some(t(6)), &[row(&a, &format!("Zed{u}Knight"), 0, "Muradin")]).await;
        // B : « Zed{u}_Main » ; C : « Zed{u}XMain » (ne doit PAS sortir pour « _M »).
        insert_match(&db, Some(t(3)), &[row(&b, &format!("Zed{u}_Main"), 0, "Kerrigan")]).await;
        insert_match(&db, Some(t(3)), &[row(&c, &format!("Zed{u}XMain"), 0, "Kerrigan")]).await;

        let hits = search(&db, &format!("ZED{u}night"), 20).await.unwrap();
        let ha = hits.iter().find(|h| h.toon == a).expect("A trouvé par son ancien pseudo");
        assert!(ha.matched_old);
        assert_eq!(ha.matched_name, format!("Zed{u}Night"));
        assert_eq!(ha.name.as_deref(), Some(format!("Zed{u}Knight").as_str()));
        assert_eq!(ha.games, 2);

        let hits = search(&db, &format!("zed{u}knight"), 20).await.unwrap();
        assert!(!hits.iter().find(|h| h.toon == a).unwrap().matched_old);

        let hits = search(&db, &format!("Zed{u}_M"), 20).await.unwrap();
        assert!(hits.iter().any(|h| h.toon == b));
        assert!(!hits.iter().any(|h| h.toon == c), "« _ » doit être littéral, pas un joker");
    }

    /// Budget perf (spec : p95 < 100 ms) sur le corpus réel de la base pointée par `DATABASE_URL`.
    /// `cargo test -p storm-codex-server mesure_p95_recherche -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "mesure sur corpus réel"]
    async fn mesure_p95_recherche() {
        let Some(db) = test_pool().await else { return };
        let qs = ["ar", "ma", "ni", "xx", "ra", "el", "mat", "ubi", "knight", "kerr", "zz", "an", "ov", "ig", "mon", "ele", "sta", "tor", "pro", "da"];
        let mut ms = Vec::new();
        for q in qs {
            let t0 = std::time::Instant::now();
            let n = search(&db, q, DEFAULT_LIMIT).await.unwrap().len();
            ms.push(t0.elapsed().as_secs_f64() * 1000.0);
            eprintln!("{q:>7}: {n:>3} hits en {:.1} ms", ms[ms.len() - 1]);
        }
        ms.sort_by(f64::total_cmp);
        let p95 = ms[(ms.len() * 95).div_ceil(100) - 1];
        eprintln!("p95 = {p95:.1} ms");
        assert!(p95 < 100.0, "p95 {p95:.1} ms ≥ 100 ms");
    }

    #[tokio::test]
    async fn tri_exact_puis_prefixe_puis_sous_chaine_et_limite() {
        let Some(db) = test_pool().await else { return };
        let u = uid();
        let (ex, pre, sub) = (format!("e-{u}"), format!("p-{u}"), format!("s-{u}"));
        insert_match(&db, None, &[row(&sub, &format!("xx{u}"), 0, "Muradin")]).await;
        insert_match(&db, None, &[row(&pre, &format!("{u}zz"), 0, "Muradin")]).await;
        insert_match(&db, None, &[row(&ex, &u, 0, "Muradin")]).await;
        let toons: Vec<_> = search(&db, &u, 20).await.unwrap().into_iter().map(|h| h.toon).collect();
        assert_eq!(toons, [ex, pre, sub]);
        assert_eq!(search(&db, &u, 2).await.unwrap().len(), 2);
        assert!(search(&db, "x", 20).await.unwrap().is_empty());
    }
}
