#![allow(clippy::unwrap_used)]

use crate::{
    api_tests::{call, call_raw, test_state},
    players::testutil::{insert_match, row},
    refdata::tests::{drop_db, isolated_db},
    standalone::StandaloneState,
};
use axum::http::StatusCode;
use serde_json::json;
use std::{path::PathBuf, sync::Arc};

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
