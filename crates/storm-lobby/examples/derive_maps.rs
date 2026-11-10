//! Derives the `.s2ma hash → map name` table from a folder of archived replays.
//!
//! Principle: each replay gives (map known through the full parse, hashes of the blob). A hash is
//! kept if it appears in NO replay of another map. Hashes common to several
//! maps (shared assets) are therefore discarded, but a map can have several hashes — that is
//! the normal case, its files being republished at each patch.
//!
//! Usage: cargo run --release -p storm-lobby --example derive_maps -- <folder> > src/maps.rs

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let dir: PathBuf = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: derive_maps <dossier>"))?
        .into();

    // map → number of replays (for the coverage report), and hash → maps where it appears
    let mut replays_par_carte: BTreeMap<String, usize> = BTreeMap::new();
    let mut cartes_par_hash: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for entry in std::fs::read_dir(&dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("StormReplay") {
            continue;
        }
        let Some(filename) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let Ok(replay) = storm_replay::Replay::open(&path) else {
            continue;
        };
        let Ok(blob) = replay.battlelobby_raw() else {
            continue;
        };
        let out = storm_stats::process_replay(&path, filename);
        if out.status != 1 {
            continue;
        }
        let json = out.to_json();
        let Some(carte) = json["match"]["map"].as_str() else {
            continue;
        };
        let carte = carte.to_string();

        *replays_par_carte.entry(carte.clone()).or_default() += 1;
        for h in storm_lobby::map_hashes(&blob) {
            cartes_par_hash.entry(h).or_default().insert(carte.clone());
        }
    }

    // Retention criterion: a hash is kept only if it was NEVER observed on another
    // map. We do NOT require it to cover all the replays of its map — Blizzard republishes the
    // `.s2ma` files at each patch, so a map carries several hashes depending on the build, and
    // requiring full coverage would eliminate precisely the most played maps. Measured
    // once, while tuning this criterion: full coverage only validated 9 maps
    // out of 19, against 19 out of 19 with the criterion kept here — that full-coverage criterion
    // no longer exists in this code, so the figure cannot be replayed.
    let mut table: Vec<(String, String)> = Vec::new();
    for (hash, cartes) in &cartes_par_hash {
        if cartes.len() != 1 {
            continue;
        }
        let Some(carte) = cartes.iter().next() else {
            continue;
        };
        table.push((hash.clone(), carte.clone()));
    }
    table.sort();

    println!("//! `.s2ma hash → map name` table, GENERATED — do not edit by hand.");
    println!("//!");
    println!(
        "//! Produced by `cargo run --release -p storm-lobby --example derive_maps -- <folder>`"
    );
    println!("//! on the box's archive. A hash is kept only if it was NEVER observed on another");
    println!(
        "//! map. The same map has several hashes: Blizzard republishes its `.s2ma` files at each"
    );
    println!("//! patch, so requiring a single hash to cover all the replays of a map would eliminate the most");
    println!("//! played maps. Measured once, while tuning this criterion: full");
    println!(
        "//! coverage only validated 9 maps out of 19; the criterion kept here (never observed"
    );
    println!("//! elsewhere) validated all 19. That full-coverage criterion no longer exists in the code — so");
    println!("//! the figure cannot be replayed, only attested by this initial derivation run.");
    println!();
    println!(
        "/// `(hash, map name)`, sorted by hash — the names are the ones `matches.map` stores."
    );
    println!("pub(crate) const MAP_BY_HASH: &[(&str, &str)] = &[");
    for (h, c) in &table {
        println!("    ({h:?}, {c:?}),");
    }
    println!("];");

    eprintln!(
        "maps covered : {}",
        table.iter().map(|(_, c)| c).collect::<BTreeSet<_>>().len()
    );
    eprintln!("hashes kept  : {}", table.len());
    eprintln!("maps seen    : {}", replays_par_carte.len());
    Ok(())
}
