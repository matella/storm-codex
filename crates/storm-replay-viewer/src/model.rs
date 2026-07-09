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
    pub casts: Vec<f64>, // instants (sec) de cast d'aptitude, triés, dédupliqués temporellement
    pub talents: Vec<TalentPick>, // picks de talent, triés par t croissant (tier = ordre de pick)
}

// US-19 : un pick de talent. `talent_id` reste `None` en V1 — ce crate est référentiel-free (le
// nom se résout côté front via dim_talents, à partir de match_players.talents).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TalentPick {
    pub t: f64,
    pub tier: i64, // 1-based, ordre de pick (pas le niveau du tier HotS)
    pub talent_id: Option<String>,
}

// US-19 : un passage de niveau d'équipe (ARAM = XP partagée → toute l'équipe monte ensemble).
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

// US-26 : samples de position minions/camps (non-héros), agressivement dédupliqués (cf.
// extract.rs) — volume potentiellement élevé, donc exclus du golden de test dédié.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MinionSample {
    pub t: f64,
    pub x: f64,
    pub y: f64,
    pub team: i64, // 0 = bleu, 1 = rouge, -1 = neutre/inconnu
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

// US-21..24 : objectif de carte STRUCTURÉ (pas de texte humain ici — mirroir de la décision
// FeedEvent : le crate reste référentiel/UI-free, le FRONT compose le texte affiché). Routage et
// extraction par carte vivent dans `crate::maps` (US-24) ; V1 ne couvre que Braxis (vagues zerg,
// best-effort — US-21) ; les autres cartes renvoient un vec vide (+ un warning pour les cartes
// "gap" connues, cf. US-7/22/23).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Objective {
    pub t: f64,
    pub kind: String, // ex. "zerg_wave" (Braxis)
    pub team: Option<i64>,
    pub value: Option<i64>, // ex. nombre d'unités zerg dans la vague
}
