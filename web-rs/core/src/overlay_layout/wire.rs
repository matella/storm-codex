//! Serde-only wire contract, also compiled by the server without browser dependencies.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const GRID_COLS: i32 = 80;
pub const GRID_ROWS: i32 = 45;
pub const MAX_WIDGETS: usize = 100;
pub const MAX_TEXT_CHARS: usize = 120;

pub fn valid_text(s: &str) -> bool {
    s.chars().count() <= MAX_TEXT_CHARS && !s.chars().any(char::is_control)
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "widget_type", content = "settings", rename_all = "kebab-case")]
pub enum WidgetSettings {
    Ticker {},
    Widget(WidgetOptions),
    Queue {},
    NowPlaying(NowPlayingOptions),
    Recap {},
    StreamPool {},
    PatchBanner {},
    RecordFlash {},
    Nemesis {},
    Goal {},
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct WidgetOptions {
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub me: Option<PlayerName>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct NowPlayingOptions {
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub mini: Option<bool>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub reveal: Option<bool>,
}

// Missing is allowed via `default`; explicit null is not a value of T.
fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct PlayerName(String);

impl TryFrom<String> for PlayerName {
    type Error = &'static str;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        if valid_text(&name) {
            Ok(Self(name))
        } else {
            Err("me must contain at most 120 characters and no control characters")
        }
    }
}

impl From<PlayerName> for String {
    fn from(name: PlayerName) -> Self {
        name.0
    }
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
#[serde(try_from = "PlacementWire")]
pub struct WidgetPlacement {
    pub grid_x: i32,
    pub grid_y: i32,
    pub grid_w: i32,
    pub grid_h: i32,
    pub z_order: i32,
    #[serde(flatten)]
    pub settings: WidgetSettings,
}

// Read the tag and settings together; a map here requires a JSON object, even for no-option widgets.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlacementWire {
    widget_type: WidgetType,
    grid_x: i32,
    grid_y: i32,
    grid_w: i32,
    grid_h: i32,
    z_order: i32,
    settings: Map<String, Value>,
}

impl TryFrom<PlacementWire> for WidgetPlacement {
    type Error = serde_json::Error;

    fn try_from(wire: PlacementWire) -> Result<Self, Self::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Empty {}

        let settings = Value::Object(wire.settings);
        let settings = match wire.widget_type {
            WidgetType::Widget => WidgetSettings::Widget(serde_json::from_value(settings)?),
            WidgetType::NowPlaying => WidgetSettings::NowPlaying(serde_json::from_value(settings)?),
            kind => {
                serde_json::from_value::<Empty>(settings)?;
                match kind {
                    WidgetType::Ticker => WidgetSettings::Ticker {},
                    WidgetType::Queue => WidgetSettings::Queue {},
                    WidgetType::Recap => WidgetSettings::Recap {},
                    WidgetType::StreamPool => WidgetSettings::StreamPool {},
                    WidgetType::PatchBanner => WidgetSettings::PatchBanner {},
                    WidgetType::RecordFlash => WidgetSettings::RecordFlash {},
                    WidgetType::Nemesis => WidgetSettings::Nemesis {},
                    WidgetType::Goal => WidgetSettings::Goal {},
                    WidgetType::Widget | WidgetType::NowPlaying => unreachable!(),
                }
            }
        };
        Ok(Self {
            grid_x: wire.grid_x,
            grid_y: wire.grid_y,
            grid_w: wire.grid_w,
            grid_h: wire.grid_h,
            z_order: wire.z_order,
            settings,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
