//! Wire types of `GET /api/cast` (mirror of `crates/storm-codex-server/src/cast/mod.rs`).
//! Unknown fields are ignored, so a server that grows a field never breaks the overlay.

use serde::{Deserialize, Serialize};

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

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct CastPlayer {
    pub name: String,
    pub tagline: String,
    pub toon: Option<String>,
}

/// A team as the server stores it, and as `PUT /api/cast/teams` takes it back.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct Team {
    pub name: String,
    #[serde(default)]
    pub logo: Option<String>,
    #[serde(default)]
    pub record: String,
    /// Always 5 entries on the server side.
    #[serde(default)]
    pub players: Vec<CastPlayer>,
    #[serde(default)]
    pub captain: Option<usize>,
}

impl Team {
    pub fn named(name: &str) -> Team {
        Team {
            name: name.into(),
            ..Team::default()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Game {
    pub number: u32,
    pub map: String,
    pub winner: Option<Side>,
    #[serde(default)]
    pub takedowns: (u32, u32),
    /// Replay summary. Read leniently: a malformed one counts as absent instead of breaking the whole
    /// series (React's check is "not null"; the two only differ on a malformed summary).
    #[serde(default, deserialize_with = "lenient")]
    pub summary: Option<GameSummary>,
}

fn lenient<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<GameSummary>, D::Error> {
    Ok(Option::<serde_json::Value>::deserialize(d)?.and_then(|v| serde_json::from_value(v).ok()))
}

/// One player line of the replay summary (`PlayerSummary` in cast/mod.rs).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct PlayerSummary {
    pub side: Option<Side>,
    pub toon: String,
    pub name: String,
    pub hero: String,
    pub kills: u32,
    pub takedowns: u32,
    pub deaths: u32,
    pub assists: u32,
    pub hero_damage: f64,
    pub siege_damage: f64,
    pub healing: f64,
}

/// Replay summary of one game. Pairs are `(blue, red)`; `None` / empty = value absent -> row removed.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct GameSummary {
    pub duration_secs: f64,
    pub winner: Side,
    pub takedowns: (u32, u32),
    #[serde(default)]
    pub team_level: Option<(u32, u32)>,
    #[serde(default)]
    pub structures: Option<(u32, u32)>,
    #[serde(default)]
    pub merc_camps: Option<(u32, u32)>,
    #[serde(default)]
    pub objectives: Option<(u32, u32)>,
    #[serde(default)]
    pub bans: (Vec<String>, Vec<String>),
    #[serde(default)]
    pub players: Vec<PlayerSummary>,
    /// `(seconds, Blue's XP lead)`; negative = Red ahead.
    #[serde(default)]
    pub xp_lead: Vec<(f64, f64)>,
}

/// `GET /api/cast/lineup`: recent heroes per player (<= 3, empty = hidden).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct HeroCount {
    pub hero: String,
    pub games: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct CastLineup {
    #[serde(default)]
    pub blue: Vec<Vec<HeroCount>>,
    #[serde(default)]
    pub red: Vec<Vec<HeroCount>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineupMode {
    #[default]
    Auto,
    A,
    B,
}

/// `GET /api/draft` (mirror of `DraftState` in api.ts, only what the overlay reads).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct DraftState {
    pub format: String,
    pub first_pick: Side,
    pub map: String,
    pub blue: DraftTeam,
    pub red: DraftTeam,
    pub score: (u32, u32),
    pub bo: u32,
    pub steps: Vec<DraftStep>,
    pub cursor: usize,
    pub assignments: Vec<Option<String>>,
    #[serde(default)]
    pub series_bans: Vec<String>,
    #[serde(default)]
    pub replay: Option<DraftReplay>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct DraftTeam {
    pub name: String,
    #[serde(default)]
    pub players: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DraftOrder {
    First,
    Second,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DraftAction {
    Ban,
    Pick,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub struct DraftStep {
    pub order: DraftOrder,
    pub action: DraftAction,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct DraftReplay {
    #[serde(default)]
    pub current: Option<usize>,
    #[serde(default)]
    pub playing: bool,
    #[serde(default)]
    pub next_at_ms: Option<f64>,
    #[serde(default)]
    pub interval_ms: f64,
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
    #[serde(default)]
    pub lineup_mode: LineupMode,
}
