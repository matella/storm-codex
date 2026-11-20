//! Shared wire contract for the layout editor and OBS renderer.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const GRID_COLS: i32 = 80;
pub const GRID_ROWS: i32 = 45;
pub const GRID_CELL_PX: i32 = 24;
pub const MAX_WIDGETS: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WidgetType {
    Ticker,
    Widget,
    Queue,
    NowPlaying,
    Recap,
    StreamPool,
    PatchBanner,
    RecordFlash,
    Nemesis,
    Goal,
}

impl WidgetType {
    pub const ALL: [Self; 10] = [
        Self::Ticker,
        Self::Widget,
        Self::Queue,
        Self::NowPlaying,
        Self::Recap,
        Self::StreamPool,
        Self::PatchBanner,
        Self::RecordFlash,
        Self::Nemesis,
        Self::Goal,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlayLayoutSummary {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlayLayout {
    #[serde(flatten)]
    pub summary: OverlayLayoutSummary,
    pub widgets: Vec<WidgetInstance>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WidgetInstance {
    pub id: String,
    #[serde(flatten)]
    pub placement: WidgetPlacement,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WidgetPlacement {
    pub widget_type: WidgetType,
    pub grid_x: i32,
    pub grid_y: i32,
    pub grid_w: i32,
    pub grid_h: i32,
    pub z_order: i32,
    /// Object; only `widget` (`me`) and `now-playing` (`mini`, `reveal`) take options in v1.
    pub settings: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveOverlayLayout {
    pub name: String,
    pub widgets: Vec<WidgetPlacement>,
}

impl From<OverlayLayout> for SaveOverlayLayout {
    fn from(layout: OverlayLayout) -> Self {
        Self {
            name: layout.summary.name,
            widgets: layout
                .widgets
                .into_iter()
                .map(|instance| instance.placement)
                .collect(),
        }
    }
}
