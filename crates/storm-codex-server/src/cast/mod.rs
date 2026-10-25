//! Régie de cast — état de série pur (aucune I/O) : équipes, pool de cartes, parties, score,
//! scène courante. Sérialisable (persistance, WS). Les types `Side`/`Format` viennent de `draft`
//! (JSON : `"blue"`/`"red"`, `"standard"`/`"normal"`/`"fearless"`).
use crate::draft::{Format, Side};
use serde::{Deserialize, Serialize};

pub mod api;
mod series;
pub mod store;
pub mod sync;

pub const SCHEMA_VERSION: i32 = 1;

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

/// Forme tolérante en lecture : un `status` absent donne `Available`, mais un `status`/`by`
/// invalide est une erreur (jamais dégradé silencieusement en `Available`).
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
        Ok(Self { name: r.name, status })
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

/// Résumé d'une partie (défini en détail à la tâche 9).
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct GameSummary {}

#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GameRecord {
    pub number: u8,
    pub map: String,
    pub winner: Option<Side>,
    /// [blue, red].
    pub takedowns: [u32; 2],
    pub duration_secs: Option<u32>,
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
}
impl Default for SlideState {
    fn default() -> Self {
        Self { game: None, index: 0, auto: false, interval_ms: 12_000 }
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Series {
    pub schema_version: i32,
    pub active: bool,
    pub blue: Team,
    pub red: Team,
    pub bo: u8,
    pub format: Format,
    /// [blue, red] — remplace le score calculé quand présent.
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
    #[error("winner already set")]
    WinnerAlreadySet,
    #[error("map already played")]
    MapAlreadyPlayed,
    #[error("series inactive")]
    SeriesInactive,
}
