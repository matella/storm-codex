//! Cast control room — pure series state (no I/O): teams, map pool, games, score,
//! current scene. Serialisable (persistence, WS). The `Side`/`Format` types come from `draft`
//! (JSON: `"blue"`/`"red"`, `"standard"`/`"normal"`/`"fearless"`).
use crate::draft::{Format, Side};
use serde::{Deserialize, Serialize};

pub mod api;
pub mod lineup;
pub mod replay;
mod series;
pub use series::Orientation;
pub mod store;
pub mod summary;
pub mod sync;

pub const SCHEMA_VERSION: i32 = 1;

/// The integration tests share the `cast_live` row: we serialise them (api.rs, replay.rs).
#[cfg(test)]
pub(crate) static TEST_DB_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scene {
    Lineups,
    Series,
    Draft,
    Ingame,
    #[default]
    Hide,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum MapStatus {
    #[default]
    Available,
    Banned {
        by: Side,
    },
    Picked {
        by: Side,
    },
    Decider,
}

#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(try_from = "PoolMapRaw")]
pub struct PoolMap {
    pub name: String,
    #[serde(flatten)]
    pub status: MapStatus,
}

/// Lenient shape on read: a missing `status` gives `Available`, but an invalid `status`/`by`
/// is an error (never silently degraded to `Available`).
#[derive(Deserialize)]
struct PoolMapRaw {
    #[serde(default)]
    name: String,
    #[serde(flatten)]
    rest: serde_json::Map<String, serde_json::Value>,
}
impl TryFrom<PoolMapRaw> for PoolMap {
    type Error = serde_json::Error;
    fn try_from(r: PoolMapRaw) -> Result<Self, Self::Error> {
        let status = if r.rest.contains_key("status") {
            serde_json::from_value(serde_json::Value::Object(r.rest))?
        } else {
            MapStatus::default()
        };
        Ok(Self {
            name: r.name,
            status,
        })
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Player {
    pub name: String,
    pub tagline: String,
    pub toon: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Team {
    pub name: String,
    pub logo: Option<String>,
    pub record: String,
    pub players: [Player; 5],
    pub captain: Option<u8>,
}

/// Player row of the summary (`players[toon].gameStats` values of storm-stats, see `summary.rs`).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct PlayerSummary {
    pub side: Side,
    /// storm-stats ToonHandle (key of `players`): link to the series' players (R14).
    pub toon: String,
    pub name: String,
    pub hero: String,
    /// `SoloKill` (column K of the match sheet).
    pub kills: u32,
    pub takedowns: u32,
    pub deaths: u32,
    pub assists: u32,
    pub hero_damage: u64,
    pub siege_damage: u64,
    pub healing: u64,
}

/// Compact summary of a game, extracted from a Custom replay (`POST /api/cast/games/{n}/replay`).
/// `[blue, red]` arrays (replay team 0 = Blue). `None` / empty = value missing from the storm-stats
/// output: the screen removes the row, nothing is invented (mapping: docs/spec/03-storm-stats.md).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct GameSummary {
    /// Map of the replay (storm-stats name, e.g. "Tomb of the Spider Queen"). Used by the bulk import.
    #[serde(default)]
    pub map: Option<String>,
    /// Date of the game (ISO 8601 UTC, storm-stats `date`): order of the bulk import.
    #[serde(default)]
    pub played_at: Option<String>,
    pub duration_secs: u32,
    pub winner: Side,
    pub takedowns: [u32; 2],
    #[serde(default)]
    pub team_level: Option<[u32; 2]>,
    /// Forts + keeps destroyed by the team.
    #[serde(default)]
    pub structures: Option<[u32; 2]>,
    #[serde(default)]
    pub merc_camps: Option<[u32; 2]>,
    #[serde(default)]
    pub objectives: Option<[u32; 2]>,
    /// Canonical names of the banned heroes.
    #[serde(default)]
    pub bans: [Vec<String>; 2],
    /// Blue then Red players, pick order.
    #[serde(default)]
    pub players: Vec<PlayerSummary>,
    /// (seconds, Blue's XP lead) — negative = Red ahead.
    #[serde(default)]
    pub xp_lead: Vec<(u32, i32)>,
}

/// Lenient read of a persisted summary: unreadable → `None` (the series stays loadable).
fn lenient_summary<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<GameSummary>, D::Error> {
    let v = Option::<serde_json::Value>::deserialize(d)?;
    Ok(v.and_then(|v| serde_json::from_value(v).ok()))
}

#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GameRecord {
    pub number: u8,
    pub map: String,
    pub winner: Option<Side>,
    /// [blue, red].
    pub takedowns: [u32; 2],
    pub duration_secs: Option<u32>,
    #[serde(deserialize_with = "lenient_summary")]
    pub summary: Option<GameSummary>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineupMode {
    #[default]
    Auto,
    A,
    B,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SlideState {
    pub game: Option<u8>,
    pub index: u8,
    pub auto: bool,
    pub interval_ms: u32,
    /// Time (ms since the epoch, server clock) at which slide `index` was set: the auto
    /// rotation of the overlay and of the preview is computed from this anchor (same clock → same slide).
    pub since_ms: i64,
}
impl Default for SlideState {
    fn default() -> Self {
        Self {
            game: None,
            index: 0,
            auto: false,
            interval_ms: 12_000,
            since_ms: 0,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Series {
    pub schema_version: i32,
    /// Series identity (uuid, set by `POST /api/cast/series`; empty before the first one):
    /// a long operation (replay drop) checks that it still applies to the same series.
    pub series_id: String,
    pub active: bool,
    pub blue: Team,
    pub red: Team,
    pub bo: u8,
    pub format: Format,
    /// [blue, red] — replaces the computed score when present.
    pub score_override: Option<[u8; 2]>,
    pub pool: Vec<PoolMap>,
    pub games: Vec<GameRecord>,
    pub scene: Scene,
    pub lineup_mode: LineupMode,
    pub slide: SlideState,
}

impl Series {
    pub fn new() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            series_id: String::new(),
            active: false,
            blue: Team::default(),
            red: Team::default(),
            bo: 5,
            format: Format::Fearless,
            score_override: None,
            pool: Vec::new(),
            games: Vec::new(),
            scene: Scene::Hide,
            lineup_mode: LineupMode::Auto,
            slide: SlideState::default(),
        }
    }
}
impl Default for Series {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, thiserror::Error)]
pub enum CastError {
    #[error("unknown map")]
    UnknownMap,
    #[error("map not in pool")]
    NotInPool,
    #[error("a decider map is already set")]
    DuplicateDecider,
    #[error("map not picked or already played")]
    MapNotChosen,
    #[error("unknown game")]
    UnknownGame,
    /// The winner comes from the replay's summary (which carries its own orientation): not editable by hand.
    #[error("winner already set by the game's replay")]
    WinnerAlreadySet,
    /// Only the last game can be removed (R21).
    #[error("only the last game can be removed")]
    NotLastGame,
    /// Game with a winner or a summary: not removable (R21).
    #[error("game already has a result")]
    GameHasResult,
    #[error("map already played")]
    MapAlreadyPlayed,
    #[error("series inactive")]
    SeriesInactive,
    /// Replay without proof of orientation whose winner contradicts the entered winner (R16).
    #[error("replay sides do not match the recorded winner")]
    SideMismatch,
    /// The series was replaced during a long operation (decoding of a replay).
    #[error("the series changed while the replay was decoded")]
    SeriesChanged,
}

impl CastError {
    /// Stable code returned by the routes (`409 {"error": code}`) — API contract (docs/spec/05-api.md),
    /// independent of the Rust name of the variant.
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnknownMap => "UnknownMap",
            Self::NotInPool => "NotInPool",
            Self::DuplicateDecider => "DuplicateDecider",
            Self::MapNotChosen => "MapNotChosen",
            Self::UnknownGame => "UnknownGame",
            Self::WinnerAlreadySet => "WinnerAlreadySet",
            Self::NotLastGame => "NotLastGame",
            Self::GameHasResult => "GameHasResult",
            Self::MapAlreadyPlayed => "MapAlreadyPlayed",
            Self::SeriesInactive => "SeriesInactive",
            Self::SideMismatch => "side_mismatch",
            Self::SeriesChanged => "series_changed",
        }
    }
}
