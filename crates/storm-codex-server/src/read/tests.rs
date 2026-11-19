#![allow(clippy::unwrap_used)]

use crate::{
    api_tests::{app, call, call_raw, test_state},
    players::testutil::{insert_match, row},
    refdata::tests::{drop_db, isolated_db},
    standalone::StandaloneState,
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::json;
use std::{path::PathBuf, sync::Arc};
use tower::util::ServiceExt;

#[tokio::test]
async fn matches_and_csv_share_one_perspective_for_opposing_accounts() {
    let Some(mut state) = test_state(None).await else {
        return;
    };
    let (db, db_name) = isolated_db().await.unwrap();
    state.db = db.clone();
    let manual = "1-Hero-1-1";
    let verified = "1-Hero-1-2";
    let second = "1-Hero-1-3";
    let folder = |toon| {
        PathBuf::from(format!(
            "Heroes of the Storm/Accounts/100/{toon}/Replays/Multiplayer"
        ))
    };
    let standalone = Arc::new(StandaloneState::new(&state.cfg));
    standalone.set_folders(&[folder(second), folder(verified)]);
    state.standalone = Some(standalone);
    sqlx::query("UPDATE app_settings SET value = $1 WHERE key = 'operator_names'")
        .bind(json!(["Manual"]))
        .execute(&db)
        .await
        .unwrap();
    let id = insert_match(
        &db,
        None,
        &[
            row(manual, "Manual", 0, "Valla"),
            row(verified, "OldAlias", 1, "Jaina"),
            row(second, "Second", 1, "Muradin"),
        ],
    )
    .await;
    sqlx::query(
        "UPDATE match_players SET data = data || $1 WHERE match_id = $2 AND toon_handle = $3",
    )
    .bind(json!({"gameStats": {"awards": ["EndOfMatchAwardMVPBoolean"]}}))
    .bind(id)
    .bind(manual)
    .execute(&db)
    .await
    .unwrap();

    for (query, included, perspective) in [
        ("", true, Some(verified)),
        ("result=loss", true, Some(verified)),
        ("result=win", false, None),
        ("mvp=true", false, None),
        ("account=mAnUaL&result=win&mvp=true", true, Some(manual)),
        ("account=Manual&result=loss", false, None),
        ("account=toon%3A1-Hero-1-3&result=loss", true, Some(second)),
        (
            "account=toon%3A1-Hero-1-2&result=loss",
            true,
            Some(verified),
        ),
        ("account=toon%3A1-Hero-1-1", false, None),
    ] {
        let (status, list) = call(&state, "GET", &format!("/api/matches?{query}"), None).await;
        assert_eq!(status, StatusCode::OK, "{query}");
        assert_eq!(
            list.as_array().unwrap().len(),
            usize::from(included),
            "{query}"
        );
        if included {
            assert_eq!(list[0]["id"], id, "{query}");
            assert_eq!(list[0]["perspective_toon"], json!(perspective), "{query}");
        }
        let (status, bytes) = call_raw(
            &state,
            "GET",
            &format!("/api/matches.csv?{query}"),
            None,
            vec![],
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{query}");
        let csv = String::from_utf8(bytes).unwrap();
        assert_eq!(csv.lines().count(), 1 + usize::from(included), "{query}");
        if included {
            assert!(
                csv.lines().nth(1).unwrap().starts_with(&format!("{id},")),
                "{query}"
            );
        }
    }
    // Plain names are still supported, but a replay with no resolved owner has no perspective.
    state.standalone = None;
    let (_, list) = call(&state, "GET", "/api/matches", None).await;
    assert_eq!(list[0]["perspective_toon"], manual);
    sqlx::query("DELETE FROM app_settings WHERE key = 'operator_names'")
        .execute(&db)
        .await
        .unwrap();
    let (_, list) = call(&state, "GET", "/api/matches", None).await;
    assert_eq!(list[0].get("perspective_toon"), Some(&json!(null)));
    drop(state);
    drop_db(db, &db_name).await;
}

/// `GET path` → (status, `X-Total-Count`, JSON body).
async fn get_counted(
    state: &crate::AppState,
    path: &str,
) -> (StatusCode, Option<i64>, serde_json::Value) {
    let resp = app(state)
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let total = resp
        .headers()
        .get("x-total-count")
        .map(|v| v.to_str().unwrap().parse().unwrap());
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        total,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

/// Ids of the CSV export for the same query (first column, header skipped).
async fn csv_ids(state: &crate::AppState, query: &str) -> Vec<i64> {
    let (status, bytes) = call_raw(
        state,
        "GET",
        &format!("/api/matches.csv?{query}"),
        None,
        vec![],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{query}");
    String::from_utf8(bytes)
        .unwrap()
        .lines()
        .skip(1)
        .map(|l| l.split(',').next().unwrap().parse().unwrap())
        .collect()
}

#[tokio::test]
async fn replay_search_filters_on_the_perspective_row() {
    let Some(mut state) = test_state(None).await else {
        return;
    };
    let (db, db_name) = isolated_db().await.unwrap();
    state.db = db.clone();
    sqlx::query("UPDATE app_settings SET value = $1 WHERE key = 'operator_names'")
        .bind(json!(["Me"]))
        .execute(&db)
        .await
        .unwrap();
    let (me, friend, foe) = ("1-Hero-1-10", "1-Hero-1-11", "1-Hero-1-12");
    let at = |h: u32| {
        chrono::DateTime::parse_from_rfc3339(&format!("2026-11-0{h}T12:00:00Z"))
            .unwrap()
            .with_timezone(&chrono::Utc)
    };
    // m1 (newest): Jaina with Friend against Foe on Braxis, 9 deaths, 3 kills, 10 takedowns, 800 s.
    let m1 = insert_match(
        &db,
        Some(at(3)),
        &[
            row(me, "Me", 0, "Jaina"),
            row(friend, "Friend", 0, "Valla"),
            row(foe, "Foe", 1, "Muradin"),
        ],
    )
    .await;
    // m2: Valla against Friend with Foe, 8 deaths, 1,200 s.
    let m2 = insert_match(
        &db,
        Some(at(2)),
        &[
            row(friend, "Friend", 0, "Jaina"),
            row(me, "Me", 1, "Valla"),
            row(foe, "Foe", 1, "Muradin"),
        ],
    )
    .await;
    // m3 (oldest): without the operator.
    let m3 = insert_match(
        &db,
        Some(at(1)),
        &[
            row(friend, "Friend", 0, "Jaina"),
            row(foe, "Foe", 1, "Valla"),
        ],
    )
    .await;
    for (id, map, len, deaths, kills, takedowns, apm) in [
        (m1, "Braxis Holdout", 800.0, 9, 3, 10, 150.0),
        (m2, "Braxis Holdout", 1200.0, 8, 5, 6, 90.0),
        (m3, "Cursed Hollow", 900.0, 1, 1, 1, 100.0),
    ] {
        sqlx::query("UPDATE matches SET map = $2, length = $3 WHERE id = $1")
            .bind(id)
            .bind(map)
            .bind(len)
            .execute(&db)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE match_players SET deaths = $3, kills = $4, takedowns = $5, apm = $6,
                    hero_damage = 50000, healing = 0, experience = 20000
             WHERE match_id = $1 AND toon_handle = $2",
        )
        .bind(id)
        .bind(me)
        .bind(deaths)
        .bind(kills)
        .bind(takedowns)
        .bind(apm)
        .execute(&db)
        .await
        .unwrap();
    }

    for (query, expected) in [
        ("", vec![m1, m2, m3]),
        ("my_hero=Jaina", vec![m1]),
        ("hero=Jaina", vec![m1, m2, m3]),
        ("with=1-Hero-1-11", vec![m1]),
        ("against=1-Hero-1-11", vec![m2]),
        ("with=1-Hero-1-12", vec![m2]),
        ("against=1-Hero-1-12", vec![m1]),
        ("with=1-Hero-1-10", vec![]),
        ("with=1-Hero-1-11&against=1-Hero-1-12", vec![m1]),
        ("map=Braxis+Holdout&min_deaths=9", vec![m1]),
        ("max_deaths=8", vec![m2]),
        ("min_deaths=8&max_deaths=8", vec![m2]),
        ("min_assists=7", vec![m1]),
        ("max_assists=1", vec![m2]),
        ("min_kills=5&min_takedowns=6", vec![m2]),
        ("min_apm=150", vec![m1]),
        (
            "min_hero_damage=50000&max_healing=0&min_xp=20000",
            vec![m1, m2],
        ),
        ("min_len=15", vec![m2, m3]),
        ("max_len=15", vec![m1, m3]),
        ("min_len=15&max_len=15.5", vec![m3]),
        (
            "map=Braxis+Holdout&min_deaths=9&with=1-Hero-1-11&result=win",
            vec![m1],
        ),
        ("min_deaths=9&result=loss", vec![]),
        ("player=1-Hero-1-10&min_len=14", vec![m2]),
        ("min_kills=", vec![m1, m2, m3]),
    ] {
        let (status, total, list) = get_counted(&state, &format!("/api/matches?{query}")).await;
        assert_eq!(status, StatusCode::OK, "{query}");
        let ids: Vec<i64> = list
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_i64().unwrap())
            .collect();
        assert_eq!(ids, expected, "{query}");
        assert_eq!(total, Some(expected.len() as i64), "{query}");
        assert_eq!(csv_ids(&state, query).await, expected, "csv {query}");
    }

    // Paging: the total ignores limit/offset, both list and CSV page the same way.
    let (_, total, list) = get_counted(&state, "/api/matches?limit=1&offset=1").await;
    assert_eq!(total, Some(3));
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["id"], m2);
    assert_eq!(csv_ids(&state, "limit=1&offset=1").await, vec![m2]);
    let (_, total, list) = get_counted(&state, "/api/matches?offset=5").await;
    assert_eq!((total, list), (Some(3), json!([])));
    let (_, _, list) = get_counted(&state, "/api/matches?limit=0&offset=-4").await;
    assert_eq!(
        list.as_array().unwrap().len(),
        1,
        "limit and offset are clamped"
    );

    // Invalid parameters name themselves; unknown stats are rejected.
    for (query, param) in [
        ("min_siege=1", "min_siege"),
        ("max_deaths=many", "max_deaths"),
        ("min_len=inf", "min_len"),
        ("with=1-Hero-1-11&against=1-Hero-1-11", "against"),
    ] {
        let (status, _, body) = get_counted(&state, &format!("/api/matches?{query}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
        assert_eq!(
            body,
            json!({"error": "bad_filter", "param": param}),
            "{query}"
        );
        let (status, _) = call_raw(
            &state,
            "GET",
            &format!("/api/matches.csv?{query}"),
            None,
            vec![],
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "csv {query}");
    }

    // Without a configured operator, perspective filters need an explicit account.
    sqlx::query("DELETE FROM app_settings WHERE key = 'operator_names'")
        .execute(&db)
        .await
        .unwrap();
    for query in ["my_hero=Jaina", "with=1-Hero-1-11", "min_deaths=1"] {
        let (status, _, body) = get_counted(&state, &format!("/api/matches?{query}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
        assert_eq!(body, json!({"error": "perspective_required"}), "{query}");
    }
    let (status, total, _) = get_counted(&state, "/api/matches?min_len=15").await;
    assert_eq!((status, total), (StatusCode::OK, Some(2)));
    let (status, total, list) = get_counted(&state, "/api/matches?account=me&my_hero=Valla").await;
    assert_eq!((status, total), (StatusCode::OK, Some(1)));
    assert_eq!(list[0]["id"], m2);
    drop(state);
    drop_db(db, &db_name).await;
}
