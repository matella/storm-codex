use serde::Serialize;

pub const VIEWER_VERSION: u32 = 7;
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
    pub levels: Vec<LevelTick>,
    pub objectives: Vec<Objective>,
    pub warnings: Vec<String>,
    pub minions: Vec<MinionSample>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    pub map_name: String,
    pub map_size: [f64; 2], // in tiles (MapSize/4096), diagnostic
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
    pub casts: Vec<f64>, // ability cast times (sec), sorted, time-deduplicated
    pub talents: Vec<TalentPick>, // talent picks, sorted by increasing t (tier = pick order)
}

// US-19: a talent pick. `talent_id` stays `None` in V1 — this crate is reference-data-free (the
// name is resolved on the front side via dim_talents, from match_players.talents).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TalentPick {
    pub t: f64,
    pub tier: i64, // 1-based, pick order (not the HotS tier level)
    pub talent_id: Option<String>,
}

// US-19: a team level-up (ARAM = shared XP → the whole team levels up together).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelTick {
    pub t: f64,
    pub team: i64, // 0 = bleu, 1 = rouge
    pub level: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Sample {
    pub t: f64,
    pub x: f64,
    pub y: f64,
    pub exact: bool,
}

// US-26: minion/camp (non-hero) position samples, aggressively deduplicated (see
// extract.rs) — potentially high volume, hence excluded from the dedicated test golden.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MinionSample {
    pub t: f64,
    pub x: f64,
    pub y: f64,
    pub team: i64, // 0 = blue, 1 = red, -1 = neutral/unknown
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
    pub structure_kind: Option<String>, // reuses the Structure.kind enum
}

// US-21..24: STRUCTURED map objective (no human text here — mirror of the
// FeedEvent decision: the crate stays reference-data/UI-free, the FRONT composes the displayed text). Routing and
// per-map extraction live in `crate::maps` (US-24); V1 only covers Braxis (zerg waves,
// best-effort — US-21); the other maps return an empty vec (+ a warning for the maps
// "gap" connues, cf. US-7/22/23).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Objective {
    pub t: f64,
    pub kind: String, // ex. "zerg_wave" (Braxis)
    pub team: Option<i64>,
    pub value: Option<i64>, // e.g. number of zerg units in the wave
}
