//! Constantes de hots-parser (`constants.js`/`attr.js`), exportées telles quelles en JSON
//! (régénérables : `node -e "...JSON.stringify(require('hots-parser/constants.js'))"`).

use serde_json::Value as J;
use std::sync::OnceLock;

pub fn replay_types() -> &'static J {
    static C: OnceLock<J> = OnceLock::new();
    C.get_or_init(|| {
        serde_json::from_str(include_str!("../data/constants.json"))
            .unwrap_or_else(|e| panic!("constants.json invalide : {e}"))
    })
}

pub fn hero_attribute() -> &'static J {
    static A: OnceLock<J> = OnceLock::new();
    A.get_or_init(|| {
        serde_json::from_str::<J>(include_str!("../data/attr.json"))
            .unwrap_or_else(|e| panic!("attr.json invalide : {e}"))["heroAttribute"]
            .clone()
    })
}

/// `ReplayTypes.<groupe>.<clé>` — entier (TrackerEvent, GameMode…).
pub fn rt_int(group: &str, key: &str) -> i64 {
    replay_types()[group][key]
        .as_i64()
        .unwrap_or_else(|| panic!("constante {group}.{key} absente"))
}

/// `ReplayTypes.<groupe>.<clé>` — chaîne (StatEventType, UnitType…).
pub fn rt_str(group: &str, key: &str) -> &'static str {
    replay_types()[group][key]
        .as_str()
        .unwrap_or_else(|| panic!("constante {group}.{key} absente"))
}

/// Statuts de processReplay (ReplayStatus de parser.js).
pub mod status {
    pub const OK: i64 = 1;
    pub const UNSUPPORTED: i64 = 0;
    pub const FAILURE: i64 = -2;
    pub const UNSUPPORTED_MAP: i64 = -3;
    pub const COMPUTER_PLAYER_FOUND: i64 = -4;
    pub const INCOMPLETE: i64 = -5;
    pub const TOO_OLD: i64 = -6;
    pub const UNVERIFIED: i64 = -7;
}
