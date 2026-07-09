use serde::Serialize;

pub const VIEWER_VERSION: u32 = 3;
pub const LOOP_OFFSET: i64 = 610;
pub const LOOPS_PER_SEC: f64 = 16.0;
pub const FIXED: f64 = 4096.0;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewerModel {
    pub meta: Meta,
    pub heroes: Vec<HeroTrack>,
    pub deaths: Vec<Death>,
    pub structures: Vec<Structure>,
    pub events: Vec<FeedEvent>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    pub map_name: String,
    pub map_size: [f64; 2], // en tuiles (MapSize/4096), diagnostic
    pub duration_sec: f64,
    pub loop_offset: i64,
    pub viewer_version: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeroTrack {
    pub player_id: i64, // playerId replay (m_controlPlayerId, 1..=10)
    pub samples: Vec<Sample>,
    pub life: Vec<Interval>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Sample {
    pub t: f64,
    pub x: f64,
    pub y: f64,
    pub exact: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Interval {
    pub from: f64,
    pub to: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Death {
    pub t: f64,
    pub x: f64,
    pub y: f64,
    pub victim_player_id: i64,
    pub killer_player_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Structure {
    pub team: i64,
    pub kind: String,
    pub x: f64,
    pub y: f64,
    pub destroyed_at: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedEvent {
    pub t: f64,
    pub kind: String, // "takedown" | "structure" | "camp" | "objective"
    pub team: Option<i64>,
    pub victim_player_id: Option<i64>,
    pub killer_player_id: Option<i64>,
    pub structure_kind: Option<String>, // réutilise l'enum Structure.kind
}
