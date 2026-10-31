//! Régie de cast — état de série pur (aucune I/O) : équipes, pool de cartes, parties, score,
//! scène courante. Sérialisable (persistance, WS). Les types `Side`/`Format` viennent de `draft`
//! (JSON : `"blue"`/`"red"`, `"standard"`/`"normal"`/`"fearless"`).
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

/// Les tests d'intégration partagent la ligne `cast_live` : on les sérialise (api.rs, replay.rs).
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

/// Ligne joueur du résumé (valeurs `players[toon].gameStats` de storm-stats, cf. `summary.rs`).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct PlayerSummary {
    pub side: Side,
    /// ToonHandle storm-stats (clé de `players`) : rattachement aux joueurs de la série (R14).
    pub toon: String,
    pub name: String,
    pub hero: String,
    /// `SoloKill` (colonne K de la fiche de match).
    pub kills: u32,
    pub takedowns: u32,
    pub deaths: u32,
    pub assists: u32,
    pub hero_damage: u64,
    pub siege_damage: u64,
    pub healing: u64,
}

/// Résumé compact d'une partie, extrait d'un replay Custom (`POST /api/cast/games/{n}/replay`).
/// Tableaux `[blue, red]` (équipe 0 du replay = Blue). `None` / vide = valeur absente de la sortie
/// storm-stats : l'écran retire la ligne, rien n'est inventé (mapping : docs/spec/03-storm-stats.md).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct GameSummary {
    /// Carte du replay (nom storm-stats, ex. « Tomb of the Spider Queen »). Sert à l'import groupé.
    #[serde(default)]
    pub map: Option<String>,
    /// Date de la partie (ISO 8601 UTC, storm-stats `date`) : ordre de l'import groupé.
    #[serde(default)]
    pub played_at: Option<String>,
    pub duration_secs: u32,
    pub winner: Side,
    pub takedowns: [u32; 2],
    #[serde(default)]
    pub team_level: Option<[u32; 2]>,
    /// Forts + keeps détruits par l'équipe.
    #[serde(default)]
    pub structures: Option<[u32; 2]>,
    #[serde(default)]
    pub merc_camps: Option<[u32; 2]>,
    #[serde(default)]
    pub objectives: Option<[u32; 2]>,
    /// Noms canoniques des héros bannis.
    #[serde(default)]
    pub bans: [Vec<String>; 2],
    /// Joueurs Blue puis Red, ordre de pick.
    #[serde(default)]
    pub players: Vec<PlayerSummary>,
    /// (secondes, avance d'XP de Blue) — négatif = Red devant.
    #[serde(default)]
    pub xp_lead: Vec<(u32, i32)>,
}

/// Lecture tolérante d'un résumé persisté : illisible → `None` (la série reste chargeable).
fn lenient_summary<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<GameSummary>, D::Error> {
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
    /// Instant (ms depuis l'epoch, horloge serveur) où la slide `index` a été posée : la rotation
    /// auto de l'overlay et de l'aperçu se calcule à partir de cette ancre (même horloge → même slide).
    pub since_ms: i64,
}
impl Default for SlideState {
    fn default() -> Self {
        Self { game: None, index: 0, auto: false, interval_ms: 12_000, since_ms: 0 }
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Series {
    pub schema_version: i32,
    /// Identité de la série (uuid, posée par `POST /api/cast/series` ; vide avant la première) :
    /// une opération longue (dépôt de replay) vérifie qu'elle s'applique encore à la même série.
    pub series_id: String,
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
    /// Le vainqueur vient du résumé du replay (qui porte sa propre orientation) : non modifiable à la main.
    #[error("winner already set by the game's replay")]
    WinnerAlreadySet,
    /// Seule la dernière partie peut être retirée (R21).
    #[error("only the last game can be removed")]
    NotLastGame,
    /// Partie avec vainqueur ou résumé : non retirable (R21).
    #[error("game already has a result")]
    GameHasResult,
    #[error("map already played")]
    MapAlreadyPlayed,
    #[error("series inactive")]
    SeriesInactive,
    /// Replay sans preuve d'orientation dont le vainqueur contredit le vainqueur saisi (R16).
    #[error("replay sides do not match the recorded winner")]
    SideMismatch,
    /// La série a été remplacée pendant une opération longue (décodage d'un replay).
    #[error("the series changed while the replay was decoded")]
    SeriesChanged,
}

impl CastError {
    /// Code stable renvoyé par les routes (`409 {"error": code}`) — contrat de l'API (docs/spec/05-api.md),
    /// indépendant du nom Rust de la variante.
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
