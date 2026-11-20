#![allow(clippy::unwrap_used)]

use serde_json::json;
use storm_web_core::overlay_layout::{
    NowPlayingOptions, OverlayLayout, SaveOverlayLayout, WidgetSettings, WidgetType, GRID_CELL_PX,
    GRID_COLS, GRID_ROWS,
};

#[test]
fn overlay_layout_wire_round_trips_and_converts_to_save_payload() {
    let wire = json!({
        "id": "aa814195-c602-4d5a-85ae-87966d821138", "name": "My scene",
        "created_at": "2026-11-20T12:00:00Z", "updated_at": "2026-11-20T12:00:00Z",
        "widgets": [{
            "id": "df325544-2e59-4dbb-8111-fcc8994b7d02", "widget_type": "now-playing",
            "grid_x": 79, "grid_y": 44, "grid_w": 1, "grid_h": 1, "z_order": -1,
            "settings": {"mini": true, "reveal": false}
        }]
    });
    let layout: OverlayLayout = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(
        layout.widgets[0].placement.settings,
        WidgetSettings::NowPlaying(NowPlayingOptions {
            mini: Some(true),
            reveal: Some(false)
        })
    );
    assert_eq!(serde_json::to_value(&layout).unwrap(), wire);
    let save = serde_json::to_value(SaveOverlayLayout::from(layout)).unwrap();
    assert!(save.get("id").is_none());
    assert!(save["widgets"][0].get("id").is_none());
    assert_eq!(
        save["widgets"][0]["settings"],
        wire["widgets"][0]["settings"]
    );
}

#[test]
fn shared_save_wire_rejects_invalid_settings() {
    for (kind, settings) in [
        ("goal", json!({"me": "Player"})),
        ("widget", json!({"me": null})),
        ("widget", json!({"me": "x\u{0085}y"})),
        ("widget", json!({"me": "é".repeat(121)})),
        ("now-playing", json!({"mini": "false"})),
        ("now-playing", json!({"reveal": null})),
        ("now-playing", json!({"mini": false, "extra": true})),
        ("now-playing", json!([true, false])),
        ("goal", json!([])),
        ("goal", json!(null)),
    ] {
        let wire = json!({"name": "Scene", "widgets": [{"widget_type": kind,
            "grid_x": 0, "grid_y": 0, "grid_w": 1, "grid_h": 1, "z_order": 0,
            "settings": settings}]});
        assert!(
            serde_json::from_value::<SaveOverlayLayout>(wire).is_err(),
            "{kind}: {settings}"
        );
    }
}

#[test]
fn overlay_layout_catalog_and_grid_keep_wire_names_and_size() {
    let names = json!([
        "ticker",
        "widget",
        "queue",
        "now-playing",
        "recap",
        "stream-pool",
        "patch-banner",
        "record-flash",
        "nemesis",
        "goal"
    ]);
    assert_eq!(serde_json::to_value(WidgetType::ALL).unwrap(), names);
    assert!(serde_json::from_value::<WidgetType>(json!("cast/overlay")).is_err());
    assert_eq!(
        (GRID_COLS * GRID_CELL_PX, GRID_ROWS * GRID_CELL_PX),
        (1920, 1080)
    );
}
