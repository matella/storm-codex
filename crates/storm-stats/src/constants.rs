//! hots-parser constants (`constants.js`/`attr.js`), exported as is to JSON
//! (regenerable: `node -e "...JSON.stringify(require('hots-parser/constants.js'))"`).

use serde_json::Value as J;
use std::sync::OnceLock;

pub fn replay_types() -> &'static J {
    static C: OnceLock<J> = OnceLock::new();
    C.get_or_init(|| {
        serde_json::from_str(include_str!("../data/constants.json"))
            .unwrap_or_else(|e| panic!("invalid constants.json: {e}"))
    })
}

pub fn hero_attribute() -> &'static J {
    static A: OnceLock<J> = OnceLock::new();
    A.get_or_init(|| {
        serde_json::from_str::<J>(include_str!("../data/attr.json"))
            .unwrap_or_else(|e| panic!("invalid attr.json: {e}"))["heroAttribute"]
            .clone()
    })
}

/// `ReplayTypes.<group>.<key>` — integer (TrackerEvent, GameMode…).
pub fn rt_int(group: &str, key: &str) -> i64 {
    replay_types()[group][key]
        .as_i64()
        .unwrap_or_else(|| panic!("constante {group}.{key} absente"))
}

/// `ReplayTypes.<group>.<key>` — string (StatEventType, UnitType…).
pub fn rt_str(group: &str, key: &str) -> &'static str {
    replay_types()[group][key]
        .as_str()
        .unwrap_or_else(|| panic!("constante {group}.{key} absente"))
}

/// processReplay statuses (ReplayStatus of parser.js).
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

#[cfg(test)]
mod tests {
    use super::hero_attribute;

    /// Xal'atath (added after hots-parser 7.55.7): attribute code `HXAL` taken from real replays.
    #[test]
    fn xalatath_attribute_maps_to_its_name() {
        assert_eq!(hero_attribute()["HXAL"].as_str(), Some("Xal'atath"));
    }
}
