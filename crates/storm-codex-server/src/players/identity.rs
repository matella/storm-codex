//! Linked accounts: similarity indices of a target player with **preselected** candidates
//! (never a pairwise comparison of all the players), then pure scoring (`similarity`).
//!
//! Exclusions: the operator (`app_settings.operator_names`) and the upload token owners
//! are in all the uploaded games; without exclusion, everyone would be "linked" through them.

use super::features::{cosine, intervals_overlap, talent_agreement};
use super::similarity::{score, PairFeatures, Suggestion};
use serde_json::Value as J;
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};

/// Minimum games of an account to be a candidate on its hero pool.
pub const MIN_CAND_GAMES: i64 = 10;
/// Maximum number of detailed candidates (budget: no pairwise comparison).
pub const MAX_CANDIDATES: usize = 30;
/// A token only has an "owner" from this number of uploaded games.
pub const TOKEN_MIN_MATCHES: usize = 50;
/// Minimum games together for a teammate to count as "recurring".
const MIN_PAIR_GAMES: u32 = 2;
/// Teammates of the target used to look for candidates "of the same circle".
const TOP_TEAMMATES: usize = 15;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Candidate {
    pub toon: String,
    pub name: Option<String>,
    #[serde(flatten)]
    pub suggestion: Suggestion,
}

/// Owners of an upload token, from the games of each account among the token's games
/// (`total` = uploaded games). The **main account** is present in ≥ 50% of
/// them; **another account of the same person** is present in ≥ 10% without ever sharing
/// a game with an already kept owner (a person does not play two accounts in the same
/// game). Frequent friends play *with* the main account or with the other account: not owners.
pub fn owners_from(per_toon: &HashMap<String, HashSet<i64>>, total: usize) -> HashSet<String> {
    let share = |n: usize| n as f64 / total.max(1) as f64;
    // tie on games → smallest toon name (deterministic)
    let Some((main, main_matches)) = per_toon
        .iter()
        .filter(|(_, m)| share(m.len()) >= 0.5)
        .max_by(|a, b| a.1.len().cmp(&b.1.len()).then(b.0.cmp(a.0)))
    else {
        return HashSet::new();
    };
    // Other accounts of the same person: ≥ 10% of the games, never in a game with an
    // already kept owner. We go from the biggest to the smallest: a friend who only duos with
    // the other account (≥ 10%, disjoint from the main one) plays WITH it, so is not kept.
    let mut owners = HashSet::from([main.clone()]);
    let mut taken: Vec<&HashSet<i64>> = vec![main_matches];
    let mut rest: Vec<(&String, &HashSet<i64>)> = per_toon
        .iter()
        .filter(|(t, m)| *t != main && share(m.len()) >= 0.1)
        .collect();
    rest.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(b.0)));
    for (toon, matches) in rest {
        if taken.iter().all(|t| t.is_disjoint(matches)) {
            owners.insert(toon.clone());
            taken.push(matches);
        }
    }
    owners
}

/// Games of a token: all of them, and per account.
type TokenMatches = (HashSet<i64>, HashMap<String, HashSet<i64>>);

/// `token_id → owner accounts` for the tokens with at least `min_matches` games.
pub async fn token_owners_with(
    db: &PgPool,
    min_matches: usize,
) -> Result<HashMap<i64, HashSet<String>>, sqlx::Error> {
    let rows: Vec<(i64, i64, String)> = sqlx::query_as(
        "WITH tok AS (
            SELECT token_id FROM uploads WHERE token_id IS NOT NULL AND match_id IS NOT NULL
            GROUP BY token_id HAVING count(DISTINCT match_id) >= $1
         )
         SELECT DISTINCT u.token_id, u.match_id, mp.toon_handle
         FROM uploads u JOIN tok ON tok.token_id = u.token_id
         JOIN match_players mp ON mp.match_id = u.match_id
         WHERE u.match_id IS NOT NULL",
    )
    .bind(i64::try_from(min_matches).unwrap_or(i64::MAX))
    .fetch_all(db)
    .await?;
    let mut by_token: HashMap<i64, TokenMatches> = HashMap::new();
    for (token, match_id, toon) in rows {
        let e = by_token.entry(token).or_default();
        e.0.insert(match_id);
        e.1.entry(toon).or_default().insert(match_id);
    }
    Ok(by_token
        .into_iter()
        .map(|(token, (all, per))| (token, owners_from(&per, all.len())))
        .filter(|(_, o)| !o.is_empty())
        .collect())
}

pub async fn token_owners(db: &PgPool) -> Result<HashMap<i64, HashSet<String>>, sqlx::Error> {
    token_owners_with(db, TOKEN_MIN_MATCHES).await
}

/// Accounts one of whose nicknames matches `app_settings.operator_names`.
pub async fn operator_toons(db: &PgPool) -> Result<HashSet<String>, sqlx::Error> {
    let toons: Vec<String> = sqlx::query_scalar(
        "SELECT p.toon_handle FROM players p
         WHERE EXISTS (
            SELECT 1 FROM jsonb_array_elements_text(p.names) n
            WHERE lower(n) IN (SELECT lower(jsonb_array_elements_text(value)) FROM app_settings WHERE key = 'operator_names'))",
    )
    .fetch_all(db)
    .await?;
    Ok(toons.into_iter().collect())
}

/// Operator ∪ all the token owners.
pub async fn excluded_toons(
    db: &PgPool,
    owners: &HashMap<i64, HashSet<String>>,
) -> Result<HashSet<String>, sqlx::Error> {
    let mut ex = operator_toons(db).await?;
    ex.extend(owners.values().flatten().cloned());
    Ok(ex)
}

pub async fn suggest(db: &PgPool, target: &str) -> Result<Vec<Candidate>, sqlx::Error> {
    let owners = token_owners(db).await?;
    let excluded = excluded_toons(db, &owners).await?;
    suggest_with(db, target, &excluded, &owners).await
}

/// ≤ `MAX_CANDIDATES` candidates with a suggestion, sorted by decreasing score. Excludes the target, the
/// `excluded` ones (except the target's token co-owners) and the already decided pairs.
pub async fn suggest_with(
    db: &PgPool,
    target: &str,
    excluded: &HashSet<String>,
    owners: &HashMap<i64, HashSet<String>>,
) -> Result<Vec<Candidate>, sqlx::Error> {
    let target_heroes = hero_vector(db, target).await?;
    if target_heroes.is_empty() {
        return Ok(vec![]);
    }
    let decided = decided_links(db, target).await?;
    let co = co_occurring(db, target).await?;
    let mates_t = teammates(db, target, excluded).await?;
    let profiles = super::profiles::shared_with(db, target).await?;

    // Preselection (priority order): token co-owners, "circle" (accounts that
    // share ≥ 2 recurring teammates with the target), close hero pool. Never the target,
    // never an account already met in a game (same person ≠ two accounts in a game),
    // never an already decided pair.
    let mut top_mates: Vec<(&String, &u32)> = mates_t
        .iter()
        .filter(|(_, g)| **g >= MIN_PAIR_GAMES)
        .collect();
    top_mates.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let top_mates: Vec<String> = top_mates
        .into_iter()
        .take(TOP_TEAMMATES)
        .map(|(t, _)| t.clone())
        .collect();

    // Never-candidate accounts: the target, the accounts already met in a game, the already
    // decided pairs, the operator and the token owners.
    let mut skip: HashSet<String> = excluded.clone();
    skip.extend(co.iter().cloned());
    skip.extend(decided.keys().cloned());
    skip.insert(target.to_owned());

    // Token co-owners: candidates even if they are excluded otherwise (except already met/decided).
    let mut co_owners: Vec<String> = owners
        .values()
        .filter(|o| o.contains(target))
        .flatten()
        .filter(|t| *t != target && !co.contains(*t) && !decided.contains_key(*t))
        .cloned()
        .collect();
    co_owners.sort();
    co_owners.dedup();
    // Accounts with the same profile name: candidates even if they are excluded otherwise (operator,
    // token owners), except already met/decided; first, it is the strongest signal.
    let mut prof_cands: Vec<String> = profiles
        .keys()
        .filter(|t| !co.contains(*t) && !decided.contains_key(*t))
        .cloned()
        .collect();
    prof_cands.sort();
    prof_cands.extend(co_owners);
    let co_owners = prof_cands;

    let circle = circle_candidates(db, &top_mates, target, &skip, 200).await?;
    let all_heroes = all_hero_vectors(db).await?;
    let mut by_cos: Vec<(f32, &String)> = all_heroes
        .iter()
        .filter(|(t, _)| !skip.contains(*t))
        .map(|(t, v)| (cosine(&target_heroes, v), t))
        .filter(|(c, _)| *c >= 0.5)
        .collect();
    by_cos.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(b.1)));
    let hero: Vec<String> = by_cos
        .into_iter()
        .take(MAX_CANDIDATES)
        .map(|(_, t)| t.clone())
        .collect();
    let ordered = merge_paths(co_owners, circle, hero, MAX_CANDIDATES);

    let windows_t = match_windows(db, target).await?;
    let hours_t = hour_histogram(db, target).await?;
    let talents_t = talent_rows(db, target).await?;
    let games_a = total_games(&target_heroes);

    let mut out = Vec::new();
    for cand in ordered {
        let heroes_c = match all_heroes.get(&cand) {
            Some(v) => v.clone(),
            None => hero_vector(db, &cand).await?,
        };
        let mates_c = teammates(db, &cand, excluded).await?;
        let (mut shared, mut shared_games) = (0u32, 0u32);
        for (t, gt) in &mates_t {
            if let Some(gc) = mates_c
                .get(t)
                .filter(|g| **g >= MIN_PAIR_GAMES && *gt >= MIN_PAIR_GAMES)
            {
                shared += 1;
                shared_games += (*gt).min(*gc);
            }
        }
        let features = PairFeatures {
            games_a,
            games_b: total_games(&heroes_c),
            hero_cosine: cosine(&target_heroes, &heroes_c),
            shared_teammates: shared,
            shared_teammate_games: shared_games,
            talent_agreement: talent_agreement(&talents_t, &talent_rows(db, &cand).await?),
            hour_cosine: cosine(&hours_t, &hour_histogram(db, &cand).await?),
            overlapping_games: intervals_overlap(&windows_t, &match_windows(db, &cand).await?),
            same_token_owner: owners
                .values()
                .any(|o| o.contains(target) && o.contains(&cand)),
            shared_profiles: profiles.get(&cand).cloned().unwrap_or_default(),
        };
        if let Some(suggestion) = score(&features) {
            let name = super::names::current_name(db, &cand).await?;
            out.push(Candidate {
                toon: cand,
                name,
                suggestion,
            });
        }
    }
    out.sort_by(|a, b| {
        b.suggestion
            .score
            .cmp(&a.suggestion.score)
            .then(a.toon.cmp(&b.toon))
    });
    Ok(out)
}

/// Merges the (already filtered) preselection paths into at most `cap` accounts: token
/// co-owners first, then alternating circle / hero pool so that no path starves the other.
fn merge_paths(
    owners: Vec<String>,
    circle: Vec<String>,
    hero: Vec<String>,
    cap: usize,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(cap);
    let mut seen: HashSet<String> = HashSet::new();
    let (mut c, mut h) = (circle.into_iter(), hero.into_iter());
    let mut take = |t: String, out: &mut Vec<String>| {
        if out.len() < cap && seen.insert(t.clone()) {
            out.push(t);
        }
    };
    for t in owners {
        take(t, &mut out);
    }
    loop {
        let (a, b) = (c.next(), h.next());
        if a.is_none() && b.is_none() || out.len() >= cap {
            break;
        }
        for t in a.into_iter().chain(b) {
            take(t, &mut out);
        }
    }
    out
}

fn total_games(heroes: &HashMap<String, u32>) -> u32 {
    heroes.values().sum()
}

fn to_u32(n: i64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

async fn hero_vector(db: &PgPool, toon: &str) -> Result<HashMap<String, u32>, sqlx::Error> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT hero, count(*) FROM match_players WHERE toon_handle = $1 AND hero IS NOT NULL GROUP BY hero",
    )
    .bind(toon)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(|(h, n)| (h, to_u32(n))).collect())
}

/// Hero vectors of all the accounts with at least `MIN_CAND_GAMES` games, in one query.
async fn all_hero_vectors(
    db: &PgPool,
) -> Result<HashMap<String, HashMap<String, u32>>, sqlx::Error> {
    let rows: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT toon_handle, hero, count(*) FROM match_players
         WHERE hero IS NOT NULL
           AND toon_handle IN (SELECT toon_handle FROM match_players GROUP BY toon_handle HAVING count(*) >= $1)
         GROUP BY toon_handle, hero",
    )
    .bind(MIN_CAND_GAMES)
    .fetch_all(db)
    .await?;
    let mut out: HashMap<String, HashMap<String, u32>> = HashMap::new();
    for (toon, hero, n) in rows {
        out.entry(toon).or_default().insert(hero, to_u32(n));
    }
    Ok(out)
}

/// All the accounts met at least once in a game with `toon` (team or opponent).
async fn co_occurring(db: &PgPool, toon: &str) -> Result<HashSet<String>, sqlx::Error> {
    let v: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT o.toon_handle FROM match_players t
         JOIN match_players o ON o.match_id = t.match_id AND o.toon_handle <> t.toon_handle
         WHERE t.toon_handle = $1",
    )
    .bind(toon)
    .fetch_all(db)
    .await?;
    Ok(v.into_iter().collect())
}

/// Teammates (same team) of `toon` and number of games together, excluding `excluded`.
async fn teammates(
    db: &PgPool,
    toon: &str,
    excluded: &HashSet<String>,
) -> Result<HashMap<String, u32>, sqlx::Error> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT o.toon_handle, count(*) FROM match_players t
         JOIN match_players o ON o.match_id = t.match_id AND o.team = t.team AND o.toon_handle <> t.toon_handle
         WHERE t.toon_handle = $1 GROUP BY o.toon_handle",
    )
    .bind(toon)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .filter(|(t, _)| !excluded.contains(t))
        .map(|(t, n)| (t, to_u32(n)))
        .collect())
}

/// Accounts sharing at least 2 of the given recurring teammates, excluding `skip` (filtered **before** the
/// limit: otherwise accounts to discard would take the places and hide real candidates).
async fn circle_candidates(
    db: &PgPool,
    mates: &[String],
    target: &str,
    skip: &HashSet<String>,
    limit: i64,
) -> Result<Vec<String>, sqlx::Error> {
    if mates.len() < 2 {
        return Ok(vec![]);
    }
    sqlx::query_scalar(
        "SELECT b.toon_handle FROM match_players a
         JOIN match_players b ON b.match_id = a.match_id AND b.team = a.team AND b.toon_handle <> a.toon_handle
         WHERE a.toon_handle = ANY($1) AND b.toon_handle <> $2 AND NOT (b.toon_handle = ANY($3))
         GROUP BY b.toon_handle HAVING count(DISTINCT a.toon_handle) >= 2
         ORDER BY count(DISTINCT a.toon_handle) DESC, b.toon_handle LIMIT $4",
    )
    .bind(mates)
    .bind(target)
    .bind(skip.iter().cloned().collect::<Vec<_>>())
    .bind(limit)
    .fetch_all(db)
    .await
}

/// `(start, end)` windows in epoch seconds of each dated game of the account.
async fn match_windows(db: &PgPool, toon: &str) -> Result<Vec<(i64, i64)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT extract(epoch FROM m.played_at)::bigint,
                extract(epoch FROM m.played_at)::bigint + COALESCE(m.length, 0)::bigint
         FROM match_players mp JOIN matches m ON m.id = mp.match_id
         WHERE mp.toon_handle = $1 AND m.played_at IS NOT NULL",
    )
    .bind(toon)
    .fetch_all(db)
    .await
}

/// Games per UTC hour (0..23).
async fn hour_histogram(db: &PgPool, toon: &str) -> Result<HashMap<i32, u32>, sqlx::Error> {
    let rows: Vec<(i32, i64)> = sqlx::query_as(
        "SELECT extract(hour FROM m.played_at AT TIME ZONE 'UTC')::int, count(*)
         FROM match_players mp JOIN matches m ON m.id = mp.match_id
         WHERE mp.toon_handle = $1 AND m.played_at IS NOT NULL GROUP BY 1",
    )
    .bind(toon)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(|(h, n)| (h, to_u32(n))).collect())
}

async fn talent_rows(db: &PgPool, toon: &str) -> Result<Vec<(String, J)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT hero, data -> 'talents' FROM match_players
         WHERE toon_handle = $1 AND hero IS NOT NULL AND jsonb_typeof(data -> 'talents') = 'object'",
    )
    .bind(toon)
    .fetch_all(db)
    .await
}

/// `other account → status` of the pairs already decided with `toon`.
async fn decided_links(db: &PgPool, toon: &str) -> Result<HashMap<String, String>, sqlx::Error> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT CASE WHEN toon_a = $1 THEN toon_b ELSE toon_a END, status
         FROM player_links WHERE toon_a = $1 OR toon_b = $1",
    )
    .bind(toon)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().collect())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::players::similarity::Level;
    use crate::players::testutil::*;
    use chrono::{Duration, TimeZone, Utc};

    fn set(ids: &[i64]) -> HashSet<i64> {
        ids.iter().copied().collect()
    }

    #[test]
    fn proprietaires_principal_et_autre_compte_jamais_avec_lui_mais_pas_les_amis() {
        // 20 uploaded games. M (main) in 1..=14; S (other account) in 15..=20, never
        // with M; F (friend) in 1..=6, with M; R (rare) in 15 only.
        let mut per: HashMap<String, HashSet<i64>> = HashMap::new();
        per.insert("M".into(), set(&(1..=14).collect::<Vec<_>>()));
        per.insert("S".into(), set(&(15..=20).collect::<Vec<_>>()));
        per.insert("F".into(), set(&(1..=6).collect::<Vec<_>>()));
        per.insert("R".into(), set(&[15]));
        let o = owners_from(&per, 20);
        assert!(o.contains("M") && o.contains("S"));
        assert!(
            !o.contains("F"),
            "a friend who plays with the main account is not an owner"
        );
        assert!(!o.contains("R"), "below 10% of the games");
    }

    #[test]
    fn pas_de_principal_pas_de_proprietaire() {
        // Nobody exceeds 50% → shared token / without a clear owner.
        let mut per: HashMap<String, HashSet<i64>> = HashMap::new();
        per.insert("A".into(), set(&[1, 2, 3]));
        per.insert("B".into(), set(&[4, 5, 6]));
        assert!(owners_from(&per, 10).is_empty());
    }

    #[test]
    fn lami_qui_duo_avec_lautre_compte_nest_pas_proprietaire() {
        // M (main) in 1..=14; S (other account) in 15..=20; F (friend who only duos with S)
        // in 15..=18: disjoint from M, ≥ 10% — but he plays WITH S, so not the same person as M.
        let mut per: HashMap<String, HashSet<i64>> = HashMap::new();
        per.insert("M".into(), set(&(1..=14).collect::<Vec<_>>()));
        per.insert("S".into(), set(&(15..=20).collect::<Vec<_>>()));
        per.insert("F".into(), set(&(15..=18).collect::<Vec<_>>()));
        let o = owners_from(&per, 20);
        assert_eq!(o, HashSet::from(["M".to_owned(), "S".to_owned()]));
    }

    #[test]
    fn merge_paths_reserve_des_places_pour_chaque_chemin_et_borne() {
        let circle: Vec<String> = (0..40).map(|i| format!("c{i}")).collect();
        let hero: Vec<String> = (0..5).map(|i| format!("h{i}")).collect();
        let m = merge_paths(vec!["o1".into()], circle, hero, 30);
        assert_eq!(m.len(), 30);
        assert_eq!(m[0], "o1", "the token co-owners come first");
        assert_eq!(
            m.iter().filter(|t| t.starts_with('h')).count(),
            5,
            "the hero pool is not starved by the circle"
        );
        assert_eq!(
            m.iter().collect::<HashSet<_>>().len(),
            m.len(),
            "no duplicate"
        );
        // an empty path leaves all the room to the other one
        let only: Vec<String> = (0..40).map(|i| format!("c{i}")).collect();
        assert_eq!(merge_paths(vec![], only, vec![], 30).len(), 30);
    }

    #[tokio::test]
    async fn circle_candidates_filtre_avant_la_limite() {
        let Some(db) = test_pool().await else { return };
        let w = smurf_world(&db).await;
        let (d1, d2) = (format!("t-d1{}", uid()), format!("t-d2{}", uid()));
        let t0 = Utc.with_ymd_and_hms(2025, 6, 1, 20, 0, 0).unwrap();
        let fr: Vec<&str> = w.friends.iter().map(String::as_str).collect();
        // two decoys who play with the friends, more often than the smurf
        games(&db, &d1, &fr, &w.opp, 25, t0).await;
        games(&db, &d2, &fr, &w.opp, 24, t0).await;
        let mates: Vec<String> = w.friends.iter().take(2).cloned().collect();
        let skip = HashSet::from([d1.clone(), d2.clone()]);
        // limit 1: without filtering in SQL, the better-ranked decoy would take the only place
        let got = circle_candidates(&db, &mates, "toon-absent", &skip, 1)
            .await
            .unwrap();
        assert_eq!(got.len(), 1);
        assert!(
            !skip.contains(&got[0]),
            "an account to discard took the place: filtering after the limit"
        );
    }

    #[tokio::test]
    async fn token_owners_with_lit_les_uploads() {
        let Some(db) = test_pool().await else { return };
        let (u, tok) = (uid(), make_token(&db).await);
        let (m, s) = (format!("t-m{u}"), format!("t-s{u}"));
        let t = |d| Utc.with_ymd_and_hms(2025, 1, d, 20, 0, 0).unwrap();
        for d in 1..=7 {
            let id = insert_match(&db, Some(t(d)), &[row(&m, "Main", 0, "Muradin")]).await;
            link_upload(&db, tok, id).await;
        }
        for d in 8..=10 {
            let id = insert_match(&db, Some(t(d)), &[row(&s, "Smurf", 0, "Muradin")]).await;
            link_upload(&db, tok, id).await;
        }
        let owners = token_owners_with(&db, 10).await.unwrap();
        assert_eq!(
            owners.get(&tok),
            Some(&HashSet::from([m.clone(), s.clone()]))
        );
        assert!(
            !token_owners_with(&db, 11).await.unwrap().contains_key(&tok),
            "below the games threshold"
        );
    }

    // ── preselection scenarios ─────────────────────────────────────────────

    fn find<'a>(c: &'a [Candidate], toon: &str) -> Option<&'a Candidate> {
        c.iter().find(|c| c.toon == toon)
    }

    #[tokio::test]
    async fn smurf_evident_est_suggere_en_haut() {
        let Some(db) = test_pool().await else { return };
        let w = smurf_world(&db).await;
        let c = suggest_with(&w.db, &w.target, &HashSet::new(), &HashMap::new())
            .await
            .unwrap();
        let s = find(&c, &w.smurf).expect("the obvious smurf must be suggested");
        assert_eq!(s.suggestion.level, Level::High);
        assert!(find(&c, &w.target).is_none(), "never the target itself");
        assert!(c.len() <= MAX_CANDIDATES);
        assert!(
            c.windows(2)
                .all(|p| p[0].suggestion.score >= p[1].suggestion.score),
            "sorted by decreasing score"
        );
    }

    #[tokio::test]
    async fn comptes_dans_la_meme_partie_sont_exclus() {
        let Some(db) = test_pool().await else { return };
        let w = smurf_world(&db).await;
        // a game where both accounts are present (opposite teams)
        insert_match(
            &w.db,
            None,
            &[
                row(&w.target, "N", 0, "Muradin"),
                row(&w.smurf, "N", 1, "Muradin"),
            ],
        )
        .await;
        let c = suggest_with(&w.db, &w.target, &HashSet::new(), &HashMap::new())
            .await
            .unwrap();
        assert!(find(&c, &w.smurf).is_none());
    }

    #[tokio::test]
    async fn parties_qui_se_chevauchent_dans_le_temps_sont_exclues() {
        let Some(db) = test_pool().await else { return };
        let w = smurf_world(&db).await;
        // the smurf plays a game (other players) during a game of the target (same hour)
        let t0 = Utc.with_ymd_and_hms(2025, 3, 1, 20, 0, 0).unwrap();
        insert_match(&w.db, Some(t0), &[row(&w.smurf, "N", 0, "Muradin")]).await;
        let c = suggest_with(&w.db, &w.target, &HashSet::new(), &HashMap::new())
            .await
            .unwrap();
        assert!(find(&c, &w.smurf).is_none());
    }

    #[tokio::test]
    async fn paire_ecartee_nest_plus_proposee() {
        let Some(db) = test_pool().await else { return };
        let w = smurf_world(&db).await;
        let (a, b) = if w.target < w.smurf {
            (&w.target, &w.smurf)
        } else {
            (&w.smurf, &w.target)
        };
        sqlx::query("INSERT INTO player_links (toon_a, toon_b, status) VALUES ($1,$2,'dismissed')")
            .bind(a)
            .bind(b)
            .execute(&w.db)
            .await
            .unwrap();
        let c = suggest_with(&w.db, &w.target, &HashSet::new(), &HashMap::new())
            .await
            .unwrap();
        assert!(find(&c, &w.smurf).is_none());
    }

    #[tokio::test]
    async fn lien_via_loperateur_seul_est_ignore() {
        let Some(db) = test_pool().await else { return };
        let u = uid();
        let (a, b, op) = (format!("t-a{u}"), format!("t-b{u}"), format!("t-op{u}"));
        let opp: Vec<String> = (0..4).map(|i| format!("t-o{i}{u}")).collect();
        let t0 = Utc.with_ymd_and_hms(2025, 4, 1, 20, 0, 0).unwrap();
        // A and B each play 16 games with the operator; disjoint hero pools/hours.
        games(&db, &a, &[&op], &opp, 16, t0).await;
        for i in 0..16 {
            let when = t0 + Duration::days(200 + i) + Duration::hours(-14); // 06 h UTC
            let rows = [row(&b, "B", 0, "Diablo"), row(&op, "Op", 0, "Kerrigan")];
            insert_match(&db, Some(when), &rows).await;
        }
        let excluded = HashSet::from([op.clone()]);
        let c = suggest_with(&db, &a, &excluded, &HashMap::new())
            .await
            .unwrap();
        assert!(
            find(&c, &b).is_none(),
            "sharing only the operator creates no link"
        );
        assert!(find(&c, &op).is_none(), "the operator is never a candidate");
    }

    #[tokio::test]
    async fn exclusion_de_loperateur_change_le_resultat() {
        // Same world, but the "operator" is 3 shared recurring teammates: excluded → no
        // teammates signal; not excluded → signal present (reason `teammates`).
        let Some(db) = test_pool().await else { return };
        let w = smurf_world(&db).await;
        let ex: HashSet<String> = w.friends.iter().cloned().collect();
        let with = suggest_with(&w.db, &w.target, &HashSet::new(), &HashMap::new())
            .await
            .unwrap();
        let without = suggest_with(&w.db, &w.target, &ex, &HashMap::new())
            .await
            .unwrap();
        let has_tm = |c: &[Candidate]| {
            find(c, &w.smurf)
                .is_some_and(|s| s.suggestion.reasons.iter().any(|r| r.kind == "teammates"))
        };
        assert!(has_tm(&with));
        assert!(!has_tm(&without));
        let _ = &w.opp;
    }

    #[tokio::test]
    async fn deux_proprietaires_du_meme_token_donnent_la_raison_token() {
        let Some(db) = test_pool().await else { return };
        let w = smurf_world(&db).await;
        let owners = HashMap::from([(1_i64, HashSet::from([w.target.clone(), w.smurf.clone()]))]);
        let excluded: HashSet<String> = owners.values().flatten().cloned().collect();
        let c = suggest_with(&w.db, &w.target, &excluded, &owners)
            .await
            .unwrap();
        let s = find(&c, &w.smurf).expect("token co-owner = candidate even if excluded otherwise");
        assert!(s.suggestion.reasons.iter().any(|r| r.kind == "token"));
    }

    /// Probe on the real corpus (database pointed to by `DATABASE_URL`): duration and suggestions for the
    /// biggest players and a few multi-nickname accounts. To be read by a human (threshold tuning).
    /// `cargo test -p storm-codex-server sonde_identite -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "probe on the real corpus"]
    async fn sonde_identite_corpus_reel() {
        let Some(db) = test_pool().await else { return };
        let t0 = std::time::Instant::now();
        let owners = token_owners(&db).await.unwrap();
        let excluded = excluded_toons(&db, &owners).await.unwrap();
        eprintln!(
            "owners/excluded: {:?} in {:?}",
            (owners.len(), excluded.len()),
            t0.elapsed()
        );
        let toons: Vec<String> = sqlx::query_scalar(
            "(SELECT toon_handle FROM match_players GROUP BY 1 ORDER BY count(*) DESC LIMIT 6)
             UNION ALL
             (SELECT toon_handle FROM players WHERE jsonb_array_length(names) > 2 LIMIT 6)",
        )
        .fetch_all(&db)
        .await
        .unwrap();
        for toon in toons {
            let t = std::time::Instant::now();
            let c = suggest_with(&db, &toon, &excluded, &owners).await.unwrap();
            let name = crate::players::names::current_name(&db, &toon)
                .await
                .unwrap();
            eprintln!(
                "{toon} ({name:?}) → {} suggestions in {:?}",
                c.len(),
                t.elapsed()
            );
            for s in c.iter().take(4) {
                let why: Vec<_> = s.suggestion.reasons.iter().map(|r| r.kind).collect();
                eprintln!(
                    "    {:?} {:>3} {:?} {:?} {why:?}",
                    s.suggestion.level, s.suggestion.score, s.name, s.toon
                );
            }
        }
    }
}
