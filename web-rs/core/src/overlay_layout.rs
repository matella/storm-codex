//! Shared wire contract for the layout editor and OBS renderer.

mod wire;
pub use wire::*;

pub const GRID_CELL_PX: i32 = 24;

impl WidgetType {
    pub const ALL: [Self; 9] = [
        Self::Ticker,
        Self::Widget,
        Self::Queue,
        Self::Recap,
        Self::StreamPool,
        Self::PatchBanner,
        Self::RecordFlash,
        Self::Nemesis,
        Self::Goal,
    ];
}
