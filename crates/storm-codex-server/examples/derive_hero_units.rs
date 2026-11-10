//! Derives the "tracker hero unit → canonical name" table used by the live feed.
//!
//! During a game, only the tracker is available: it names the heroes by their unit
//! (`PlayerSpawned.Hero` = `HeroMonk`, `HeroFaerieDragon`…). The rest of the app uses the parser's
//! canonical name (`Kharazim`, `Brightwing` = `dim_heroes.id`), taken from the player's attribute 4002
//! via `attr.json`. For each replay of the archive, we pair the two by `PlayerID`.
//!
//! Usage: `cargo run --release -p storm-codex-server --example derive_hero_units -- <archive_dir>`
//! Writes `crates/storm-codex-server/src/live/hero_units.json` and lists the conflicts.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::{BTreeMap, HashMap};
use storm_replay::{Replay, Value};

fn attr_code(bytes: &[u8]) -> String {
    // 4-character code (e.g. `Wiza`), possibly padded with zeros.
    let b: Vec<u8> = bytes.iter().copied().filter(|&c| c != 0).collect();
    String::from_utf8_lossy(&b).trim().to_owned()
}

fn main() {
    let dir = std::env::args()
        .nth(1)
        .expect("usage: derive_hero_units <archive_dir>");
    let attr = storm_stats::constants::hero_attribute();
    // unit → (canonical name → occurrences)
    let mut seen: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
    // Second vote, by attribute 4002 (always a canonical name, but wrong in ARAM): used to
    // correct a `details` display name that is not canonical (e.g. accents).
    let mut seen_attr: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
    let (mut ok, mut failed) = (0u32, 0u32);

    for entry in std::fs::read_dir(&dir)
        .expect("archive illisible")
        .flatten()
    {
        let path = entry.path();
        if path.extension().is_none_or(|x| x != "StormReplay") {
            continue;
        }
        let Ok(replay) = Replay::open(&path) else {
            failed += 1;
            continue;
        };
        let (Ok(tracker), Ok(attributes)) = (replay.tracker_events(), replay.attributes()) else {
            failed += 1;
            continue;
        };
        let mut unit_by_player: HashMap<i64, String> = HashMap::new();
        for ev in &tracker {
            let is_stat = ev
                .field("_event")
                .and_then(Value::as_str_lossy)
                .is_some_and(|n| n.ends_with("SStatGameEvent"));
            if !is_stat
                || ev
                    .field("m_eventName")
                    .and_then(Value::as_str_lossy)
                    .as_deref()
                    != Some("PlayerSpawned")
            {
                continue;
            }
            let get = |k: &str, key: &str| -> Option<&Value> {
                ev.field(k)?
                    .as_array()?
                    .iter()
                    .find(|e| {
                        e.field("m_key").and_then(Value::as_str_lossy).as_deref() == Some(key)
                    })?
                    .field("m_value")
            };
            if let (Some(p), Some(h)) = (
                get("m_intData", "PlayerID").and_then(Value::as_int),
                get("m_stringData", "Hero").and_then(Value::as_str_lossy),
            ) {
                unit_by_player.insert(p, h);
            }
        }
        // PlayerID → slot (SPlayerSetupEvent) → `details` player (working_set_slot_id): the
        // hero really played (attribute 4002 carries the lobby choice, wrong in ARAM).
        let mut slot_of: HashMap<i64, i64> = HashMap::new();
        for ev in &tracker {
            if ev
                .field("_event")
                .and_then(Value::as_str_lossy)
                .is_some_and(|n| n.ends_with("SPlayerSetupEvent"))
            {
                if let (Some(p), Some(Value::Int(s))) = (
                    ev.field("m_playerId").and_then(Value::as_int),
                    ev.field("m_slotId"),
                ) {
                    slot_of.insert(p, *s);
                }
            }
        }
        let Ok(details) = replay.details() else {
            failed += 1;
            continue;
        };
        for (player, unit) in unit_by_player {
            let Some(slot) = slot_of.get(&player) else {
                continue;
            };
            let Some(d) = details
                .players
                .iter()
                .find(|d| d.working_set_slot_id == Some(*slot))
            else {
                continue;
            };
            *seen
                .entry(unit.clone())
                .or_default()
                .entry(d.hero.clone())
                .or_default() += 1;
            let code = u8::try_from(player)
                .ok()
                .and_then(|s| attributes.scopes.get(&s))
                .and_then(|scope| scope.get(&4002))
                .and_then(|v| v.first())
                .map(|v| attr_code(&v.value));
            if let Some(name) = code.and_then(|c| attr.get(&c)).and_then(|n| n.as_str()) {
                *seen_attr
                    .entry(unit)
                    .or_default()
                    .entry(name.to_owned())
                    .or_default() += 1;
            }
        }
        ok += 1;
    }

    let mut table: BTreeMap<String, String> = BTreeMap::new();
    let mut conflicts = 0;
    for (unit, names) in &seen {
        // Majority vote: a small share of the replays (attribute slot ≠ PlayerID, ~2–5%)
        // pair wrongly; only a close vote (< 80% for the winner) is reported.
        let (best, n) = names.iter().max_by_key(|(_, n)| **n).unwrap();
        let total: u32 = names.values().sum();
        if f64::from(*n) < 0.8 * f64::from(total) {
            conflicts += 1;
            eprintln!("close vote {unit}: {best} {n}/{total}");
        }
        let canonical = |n: &str| attr.as_object().is_some_and(|o| o.values().any(|v| v == n));
        let name = if canonical(best) {
            best.clone()
        } else {
            let fallback = seen_attr
                .get(unit)
                .and_then(|m| m.iter().max_by_key(|(_, n)| **n))
                .map(|(k, _)| k.clone())
                .unwrap_or_else(|| best.clone());
            eprintln!("{unit}: \"{best}\" not canonical → {fallback}");
            fallback
        };
        table.insert(unit.clone(), name);
    }
    let out = concat!(env!("CARGO_MANIFEST_DIR"), "/src/live/hero_units.json");
    std::fs::write(out, serde_json::to_string_pretty(&table).unwrap() + "\n").unwrap();
    println!(
        "{ok} replays read ({failed} unreadable) — {} units, {conflicts} close votes, {} known canonical heroes → {out}",
        table.len(),
        attr.as_object().map_or(0, |o| o.len())
    );
}
