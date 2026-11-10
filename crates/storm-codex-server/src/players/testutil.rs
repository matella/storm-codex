//! Integration fixtures. Skipped without `DATABASE_URL` (like `api_tests`): run with the
//! dev Docker Postgres. Database shared between tests → always `uuid` toons/nicknames.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use chrono::{DateTime, Duration, TimeZone, Utc};
use sqlx::PgPool;

pub async fn test_pool() -> Option<PgPool> {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL missing → players integration test skipped");
        return None;
    };
    let db = sqlx::postgres::PgPoolOptions::new()
        .connect(&url)
        .await
        .expect("connexion");
    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .expect("migrations");
    Some(db)
}

pub fn uid() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

pub struct Row<'a> {
    pub toon: &'a str,
    pub name: &'a str,
    pub team: i32,
    pub hero: &'a str,
    pub talents: serde_json::Value,
}

pub fn row<'a>(toon: &'a str, name: &'a str, team: i32, hero: &'a str) -> Row<'a> {
    Row {
        toon,
        name,
        team,
        hero,
        talents: serde_json::json!({}),
    }
}

/// Inserts a game and its players (+ the `players` UPSERT like `project.rs`). Returns `matches.id`.
pub async fn insert_match(db: &PgPool, played_at: Option<DateTime<Utc>>, rows: &[Row<'_>]) -> i64 {
    let fp = format!("test-{}", uid());
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO matches (fingerprint, parser_version, data, played_at) VALUES ($1, 0, '{}', $2) RETURNING id",
    )
    .bind(&fp)
    .bind(played_at)
    .fetch_one(db)
    .await
    .unwrap();
    for r in rows {
        let data = serde_json::json!({ "name": r.name, "hero": r.hero, "talents": r.talents });
        sqlx::query(
            "INSERT INTO match_players (match_id, toon_handle, name, hero, team, win, data) VALUES ($1,$2,$3,$4,$5,$6,$7)",
        )
        .bind(id)
        .bind(r.toon)
        .bind(r.name)
        .bind(r.hero)
        .bind(r.team)
        .bind(r.team == 0)
        .bind(data)
        .execute(db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO players (toon_handle, last_name, names, updated_at) VALUES ($1,$2,jsonb_build_array($2::text),now())
             ON CONFLICT (toon_handle) DO UPDATE SET last_name = EXCLUDED.last_name,
               names = CASE WHEN players.names ? EXCLUDED.last_name THEN players.names ELSE players.names || jsonb_build_array(EXCLUDED.last_name) END",
        )
        .bind(r.toon)
        .bind(r.name)
        .execute(db)
        .await
        .unwrap();
    }
    id
}

/// Creates a fake upload token; returns its id.
pub async fn make_token(db: &PgPool) -> i64 {
    sqlx::query_scalar("INSERT INTO upload_tokens (name, token_hash) VALUES ($1, $2) RETURNING id")
        .bind(format!("test-{}", uid()))
        .bind(format!("test-hash-{}", uid()))
        .fetch_one(db)
        .await
        .unwrap()
}

/// Records that a token uploaded a game.
pub async fn link_upload(db: &PgPool, token_id: i64, match_id: i64) {
    sqlx::query("INSERT INTO uploads (token_id, filename, fingerprint, status, match_id) VALUES ($1,'t.StormReplay',$2,'parsed',$3)")
        .bind(token_id)
        .bind(format!("test-up-{}", uid()))
        .bind(match_id)
        .execute(db)
        .await
        .unwrap();
}

/// Deletes ALL the fixture data: the `test-%` games (CASCADE on their players), the
/// players who only appear in these games, the `test-%` tokens. To run by hand (never
/// during other tests); the real players appear in no `test-%` game.
/// `cargo test -p storm-codex-server purge_fixtures -- --ignored`
pub async fn purge(db: &PgPool) -> u64 {
    let mut n = 0;
    const TOONS: &str = "(SELECT mp.toon_handle FROM match_players mp JOIN matches m ON m.id = mp.match_id WHERE m.fingerprint LIKE 'test-%')";
    for q in [
        format!("DELETE FROM player_links WHERE toon_a IN {TOONS} OR toon_b IN {TOONS}"),
        format!("DELETE FROM players WHERE toon_handle IN {TOONS}"),
        "DELETE FROM uploads WHERE fingerprint LIKE 'test-%'".to_owned(),
        "DELETE FROM upload_tokens WHERE name LIKE 'test-%'".to_owned(),
        "DELETE FROM matches WHERE fingerprint LIKE 'test-%'".to_owned(),
    ] {
        n += sqlx::query(sqlx::AssertSqlSafe(q))
            .execute(db)
            .await
            .unwrap()
            .rows_affected();
    }
    n
}

#[tokio::test]
#[ignore = "manual purge of the fixtures"]
async fn purge_fixtures() {
    let Some(db) = test_pool().await else { return };
    eprintln!("{} fixture rows deleted", purge(&db).await);
}

/// `n` games where `me` (team 0) plays Muradin with `mates` (team 0) against 4 fixed
/// opponents, from `start` (one game per day, 8 pm UTC).
pub async fn games(
    db: &PgPool,
    me: &str,
    mates: &[&str],
    opp: &[String],
    n: i64,
    start: DateTime<Utc>,
) -> Vec<i64> {
    let mut ids = Vec::new();
    for i in 0..n {
        let when = start + Duration::days(i);
        let mut rows = vec![Row {
            talents: serde_json::json!({"Tier1Choice":"A","Tier2Choice":"B"}),
            ..row(me, "N", 0, "Muradin")
        }];
        rows.extend(mates.iter().map(|m| row(m, "Mate", 0, "Kerrigan")));
        rows.extend(opp.iter().map(|o| row(o, "Opp", 1, "Diablo")));
        ids.push(insert_match(db, Some(when), &rows).await);
    }
    ids
}

pub struct World {
    pub db: PgPool,
    pub target: String,
    pub smurf: String,
    pub friends: Vec<String>,
    pub opp: Vec<String>,
}

/// Target and smurf: same heroes/talents/hours, same 3 friends, never in the same game.
pub async fn smurf_world(db: &PgPool) -> World {
    let u = uid();
    let target = format!("t-tgt{u}");
    let smurf = format!("t-smu{u}");
    let friends: Vec<String> = (0..3).map(|i| format!("t-fr{i}{u}")).collect();
    let opp: Vec<String> = (0..4).map(|i| format!("t-op{i}{u}")).collect();
    let fr: Vec<&str> = friends.iter().map(String::as_str).collect();
    let t0 = Utc.with_ymd_and_hms(2025, 3, 1, 20, 0, 0).unwrap();
    games(db, &target, &fr, &opp, 18, t0).await;
    // the smurf plays on the "odd" days (shifted by a whole day + 1 h: no overlap)
    games(db, &smurf, &fr, &opp, 18, t0 + Duration::hours(24 * 100)).await;
    World {
        db: db.clone(),
        target,
        smurf,
        friends,
        opp,
    }
}
