//! Wire types of `GET /api/cast` (mirror of `crates/storm-codex-server/src/cast/mod.rs`).
//! Unknown fields are ignored, so a server that grows a field never breaks the overlay.

use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Blue,
    Red,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scene {
    Lineups,
    Series,
    Draft,
    Ingame,
    Hide,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MapStatus {
    Available,
    Banned,
    Picked,
    Decider,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PoolMap {
    pub name: String,
    pub status: MapStatus,
    /// Only present for banned / picked.
    #[serde(default)]
    pub by: Option<Side>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Team {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Game {
    pub number: u32,
    pub map: String,
    pub winner: Option<Side>,
    /// Only whether a replay summary exists matters to the slideshow.
    #[serde(default, deserialize_with = "present")]
    pub summary: bool,
}

/// `Some(anything)` -> true, `null` / absent -> false.
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    Ok(Option::<serde::de::IgnoredAny>::deserialize(d)?.is_some())
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Slide {
    pub game: Option<u32>,
    pub index: usize,
    pub auto: bool,
    pub interval_ms: u64,
    /// Anchor of the auto rotation (ms, server clock).
    pub since_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct CastSeries {
    pub active: bool,
    pub blue: Team,
    pub red: Team,
    pub bo: u32,
    pub format: String,
    pub score_override: Option<(u32, u32)>,
    pub pool: Vec<PoolMap>,
    pub games: Vec<Game>,
    pub scene: Scene,
    pub slide: Slide,
}
