#![allow(clippy::unwrap_used, clippy::expect_used)]

use crate::api_tests::{app, call, call_raw, test_state};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::{json, Value};
use tower::ServiceExt;

const ADMIN: &str = "/api/admin/overlay-layouts";
const PUBLIC: &str = "/api/overlay-layouts";

fn widget(kind: &str, settings: Value, z: i32) -> Value {
    json!({"widget_type": kind, "grid_x": 79, "grid_y": 44, "grid_w": 1,
           "grid_h": 1, "z_order": z, "settings": settings})
}

#[tokio::test]
async fn overlay_layout_crud_round_trips_and_cascades() {
    let Some(s) = test_state(None).await else {
        return;
    };
    let (status, created) = call(
        &s,
        "POST",
        ADMIN,
        Some(json!({"name": "  Stream  ", "widgets": [
            widget("widget", json!({"me": "Player"}), 5),
            widget("goal", json!({}), -1),
        ]})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["name"], "Stream");
    let id = created["id"].as_str().unwrap();
    let admin = format!("{ADMIN}/{id}");
    let public = format!("{PUBLIC}/{id}");
    let widgets = created["widgets"].as_array().unwrap();
    assert_eq!(widgets.len(), 2);
    assert_eq!(widgets[0]["settings"], json!({}));
    assert_eq!(widgets[1]["settings"], json!({"me": "Player"}));
    assert_ne!(widgets[0]["id"], widgets[1]["id"]);
    assert_eq!(
        call(&s, "GET", &public, None).await,
        (StatusCode::OK, created.clone())
    );
    assert_eq!(
        call(&s, "GET", &admin, None).await,
        (StatusCode::OK, created.clone())
    );
    let (status, list) = call(&s, "GET", ADMIN, None).await;
    assert_eq!(status, StatusCode::OK);
    let summary = list
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == id)
        .unwrap();
    assert_eq!(summary["name"], "Stream");
    assert!(summary.get("widgets").is_none());

    let (status, updated) = call(
        &s,
        "PUT",
        &admin,
        Some(json!({"name": "New", "widgets": [
            widget("goal", json!({}), 0)
        ]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["id"], id);
    assert_eq!(updated["created_at"], created["created_at"]);
    assert_ne!(updated["updated_at"], created["updated_at"]);
    assert_eq!(updated["name"], "New");
    assert_eq!(updated["widgets"].as_array().unwrap().len(), 1);
    assert_eq!(updated["widgets"][0]["widget_type"], "goal");
    assert_eq!(call(&s, "GET", &public, None).await.1, updated);
    assert_eq!(
        call(&s, "DELETE", &admin, None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(&s, "GET", &public, None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(call(&s, "GET", &admin, None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        call(&s, "DELETE", &admin, None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &s,
            "PUT",
            &admin,
            Some(json!({"name": "missing", "widgets": []}))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM overlay_widget_instance WHERE layout_id = $1::text::uuid"
        )
        .bind(id)
        .fetch_one(&s.db)
        .await
        .unwrap(),
        0
    );
}

#[tokio::test]
async fn overlay_layout_shared_wire_saves_and_decodes_public_response() {
    use super::wire::{
        OverlayLayout, PlayerName, SaveOverlayLayout, WidgetOptions, WidgetSettings,
    };

    let Some(s) = test_state(None).await else {
        return;
    };
    let payload = json!({"name": "Shared", "widgets": [
        widget("goal", json!({}), 0),
        widget("widget", json!({"me": "Zoë"}), 1),
        widget("widget", json!({}), 2),
        widget("ticker", json!({}), 3),
        widget("widget", json!({"me": "DJ"}), 4),
        widget("queue", json!({}), 5)
    ]});
    let mut draft: SaveOverlayLayout = serde_json::from_value(payload.clone()).unwrap();
    draft.widgets[4].settings = WidgetSettings::Widget(WidgetOptions {
        me: Some(PlayerName::try_from("DJ".to_string()).unwrap()),
    });
    assert_eq!(serde_json::to_value(&draft).unwrap(), payload);
    let (status, body) = call_raw(
        &s,
        "POST",
        ADMIN,
        Some("application/json"),
        serde_json::to_vec(&draft).unwrap(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&body)
    );
    let saved: OverlayLayout = serde_json::from_slice(&body).unwrap();
    assert_eq!(SaveOverlayLayout::from(saved.clone()), draft);
    let public = format!("{PUBLIC}/{}", saved.summary.id);
    let admin = format!("{ADMIN}/{}", saved.summary.id);
    let (status, raw) = call_raw(&s, "GET", &public, None, vec![]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_slice::<OverlayLayout>(&raw).unwrap(),
        saved
    );
    let (status, body) = call_raw(
        &s,
        "PUT",
        &admin,
        Some("application/json"),
        serde_json::to_vec(&SaveOverlayLayout::from(saved.clone())).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let replaced: OverlayLayout = serde_json::from_slice(&body).unwrap();
    assert_eq!(replaced.summary.id, saved.summary.id);
    for (old, new) in saved.widgets.iter().zip(&replaced.widgets) {
        assert_ne!(old.id, new.id);
    }
    assert_eq!(SaveOverlayLayout::from(replaced), draft);
    assert_eq!(
        call(&s, "DELETE", &admin, None).await.0,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn overlay_layout_storage_validates_direct_widget_settings() {
    let Some(s) = test_state(None).await else {
        return;
    };
    let mut tx = s.db.begin().await.unwrap();
    let id: String = sqlx::query_scalar(
        "INSERT INTO overlay_layout (name) VALUES ('Restore') RETURNING id::text",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    for (kind, settings, valid) in [
        ("goal", json!({}), true),
        ("goal", json!({"me": "Player"}), false),
        ("goal", json!([]), false),
        ("widget", json!({"me": "é".repeat(120)}), true),
        ("widget", json!({"me": "é".repeat(121)}), false),
        ("widget", json!({"me": "x\u{0085}y"}), false),
        ("widget", json!({"me": "x\ty"}), false),
        ("widget", json!({"me": "x\u{007f}y"}), false),
        ("widget", json!({"me": ""}), true),
        ("widget", json!({"me": null}), false),
        ("ticker", json!({}), true),
        ("ticker", json!({"extra": true}), false),
    ] {
        use sqlx::Acquire as _;
        let mut row = tx.begin().await.unwrap();
        let result = sqlx::query(
            "INSERT INTO overlay_widget_instance
               (layout_id, widget_type, grid_x, grid_y, grid_w, grid_h, z_order, settings)
               VALUES ($1::text::uuid, $2, 0, 0, 1, 1, 0, $3)",
        )
        .bind(&id)
        .bind(kind)
        .bind(&settings)
        .execute(&mut *row)
        .await;
        if valid {
            result.unwrap();
        } else {
            let error = result.unwrap_err();
            assert!(
                error
                    .as_database_error()
                    .is_some_and(|e| e.is_check_violation()),
                "{kind}: {settings}: {error}"
            );
        }
        row.rollback().await.unwrap();
    }
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn overlay_layout_read_projects_only_public_fields_and_fails_closed() {
    let Some(s) = test_state(None).await else {
        return;
    };
    // Shadow only the widget table on this connection, leaving shared test schema untouched.
    let mut tx = s.db.begin().await.unwrap();
    sqlx::query("CREATE TEMP TABLE overlay_widget_instance (LIKE public.overlay_widget_instance INCLUDING DEFAULTS) ON COMMIT DROP")
        .execute(&mut *tx).await.unwrap();
    sqlx::query("ALTER TABLE overlay_widget_instance ADD COLUMN private_note text NOT NULL DEFAULT 'internal'")
        .execute(&mut *tx).await.unwrap();
    let id: String = sqlx::query_scalar(
        "INSERT INTO overlay_layout (name) VALUES ('Projection') RETURNING id::text",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO overlay_widget_instance
               (layout_id, widget_type, grid_x, grid_y, grid_w, grid_h, z_order, settings)
               VALUES ($1::text::uuid, 'ticker', 0, 0, 1, 1, 0, '{}')",
    )
    .bind(&id)
    .execute(&mut *tx)
    .await
    .unwrap();
    let layout = serde_json::to_value(super::load(&mut tx, &id).await.ok().unwrap()).unwrap();
    let widget = &layout["widgets"][0];
    assert_eq!(widget.as_object().unwrap().len(), 8, "{widget}");
    assert_eq!(widget["settings"], json!({}));
    for settings in [
        json!({"mini": false, "extra": "private"}),
        json!({"mini": "private"}),
        json!([]),
    ] {
        sqlx::query("UPDATE overlay_widget_instance SET settings = $1")
            .bind(settings)
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(super::load(&mut tx, &id).await.is_err());
    }
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn overlay_layout_admin_routes_require_token_public_read_does_not() {
    let Some(s) = test_state(Some("test-admin")).await else {
        return;
    };
    let missing = format!("{PUBLIC}/00000000-0000-0000-0000-000000000000");
    let item = format!("{ADMIN}/00000000-0000-0000-0000-000000000000");
    for (method, path) in [
        ("GET", ADMIN),
        ("POST", ADMIN),
        ("GET", &item),
        ("PUT", &item),
        ("DELETE", &item),
    ] {
        assert_eq!(
            call(&s, method, path, None).await.0,
            StatusCode::UNAUTHORIZED,
            "{method} {path}"
        );
        let wrong = Request::builder()
            .method(method)
            .uri(path)
            .header("authorization", "Bearer wrong")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            app(&s).oneshot(wrong).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        call(&s, "GET", &missing, None).await.0,
        StatusCode::NOT_FOUND
    );
    let good = Request::post(ADMIN)
        .header("authorization", "Bearer test-admin")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"name":"On air","widgets":[]}"#))
        .unwrap();
    let response = app(&s).oneshot(good).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let created: Value = serde_json::from_slice(
        &http_body_util::BodyExt::collect(response.into_body())
            .await
            .unwrap()
            .to_bytes(),
    )
    .unwrap();
    let id = created["id"].as_str().unwrap();
    assert_eq!(
        call(&s, "GET", &format!("{PUBLIC}/{id}"), None).await,
        (StatusCode::OK, created.clone())
    );
    let delete = Request::delete(format!("{ADMIN}/{id}"))
        .header("authorization", "Bearer test-admin")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app(&s).oneshot(delete).await.unwrap().status(),
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn overlay_layout_rejects_invalid_input_without_replacing_saved_layout() {
    let Some(s) = test_state(None).await else {
        return;
    };
    let (status, original) = call(
        &s,
        "POST",
        ADMIN,
        Some(json!({"name": "Valid", "widgets": []})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = original["id"].as_str().unwrap();
    let item = format!("{ADMIN}/{id}");
    let public = format!("{PUBLIC}/{id}");
    let valid = widget("ticker", json!({}), 0);
    for (field, value) in [
        ("widget_type", json!("cast/overlay")),
        ("grid_x", json!(-1)),
        ("grid_y", json!(45)),
        ("grid_w", json!(0)),
        ("grid_w", json!(i32::MAX)),
        ("grid_h", json!(2)),
        ("grid_x", json!(0.5)),
        ("settings", json!([])),
        ("settings", json!({"me":"not for ticker"})),
    ] {
        let mut invalid = valid.clone();
        invalid[field] = value;
        let (status, body) = call(
            &s,
            "PUT",
            &item,
            Some(json!({"name":"Changed", "widgets":[valid, invalid]})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{field}: {body}");
    }
    for body in [
        json!({"name": "  ", "widgets": []}),
        json!({"name": "a".repeat(121), "widgets": []}),
        json!({"name": "x\0y", "widgets": []}),
        json!({"name":"Changed", "widgets":vec![valid; 101]}),
        json!({"name":"Changed", "widgets":[widget("widget", json!({"me": 5}), 0)]}),
        json!({"name":"Changed", "widgets":[widget("widget", json!({"me":"x\0y"}), 0)]}),
        json!({"name":"Changed"}),
    ] {
        assert_eq!(
            call(&s, "PUT", &item, Some(body)).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        call_raw(
            &s,
            "PUT",
            &item,
            Some("application/json"),
            vec![b' '; 65537]
        )
        .await
        .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        call(&s, "GET", &format!("{PUBLIC}/not-a-uuid"), None)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(call(&s, "GET", &public, None).await.1, original);
    assert_eq!(
        call(&s, "DELETE", &item, None).await.0,
        StatusCode::NO_CONTENT
    );
}
