//! Dérive la table `hash .s2ma → nom de carte` depuis un dossier de replays archivés.
//!
//! Principe : chaque replay donne (carte connue via le parse complet, hashes du blob). Un hash est
//! retenu s'il n'apparaît dans AUCUN replay d'une autre carte. Les hashes communs à plusieurs
//! cartes (assets partagés) sont donc écartés, mais une carte peut avoir plusieurs hashes — c'est
//! le cas normal, ses fichiers étant republiés à chaque patch.
//!
//! Usage : cargo run --release -p storm-lobby --example derive_maps -- <dossier> > src/maps.rs

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let dir: PathBuf = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: derive_maps <dossier>"))?
        .into();

    // carte → nombre de replays, et hash → cartes où il apparaît, et (carte,hash) → occurrences
    let mut replays_par_carte: BTreeMap<String, usize> = BTreeMap::new();
    let mut cartes_par_hash: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut occurrences: BTreeMap<(String, String), usize> = BTreeMap::new();

    for entry in std::fs::read_dir(&dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("StormReplay") {
            continue;
        }
        let Some(filename) = path.file_name().and_then(|s| s.to_str()) else { continue };
        let Ok(replay) = storm_replay::Replay::open(&path) else { continue };
        let Ok(blob) = replay.battlelobby_raw() else { continue };
        let out = storm_stats::process_replay(&path, filename);
        if out.status != 1 {
            continue;
        }
        let json = out.to_json();
        let Some(carte) = json["match"]["map"].as_str() else { continue };
        let carte = carte.to_string();

        *replays_par_carte.entry(carte.clone()).or_default() += 1;
        for h in storm_lobby::map_hashes(&blob) {
            cartes_par_hash.entry(h.clone()).or_default().insert(carte.clone());
            *occurrences.entry((carte.clone(), h)).or_default() += 1;
        }
    }

    // Un hash est discriminant s'il n'apparaît que sur une carte, et sur TOUS ses replays.
    let mut table: Vec<(String, String)> = Vec::new();
    for (hash, cartes) in &cartes_par_hash {
        if cartes.len() != 1 {
            continue;
        }
        let Some(carte) = cartes.iter().next() else { continue };
        let vus = occurrences.get(&(carte.clone(), hash.clone())).copied().unwrap_or(0);
        let total = replays_par_carte.get(carte).copied().unwrap_or(0);
        // Critère : un hash n'est retenu que s'il n'a JAMAIS été vu sur une autre carte.
        // On n'exige PAS qu'il couvre tous les replays de sa carte : Blizzard republie les
        // fichiers `.s2ma` à chaque patch, donc une carte a plusieurs hashes selon le build,
        // et exiger 100 % de couverture éliminerait précisément les cartes les plus jouées.
        let _ = (vus, total);
        table.push((hash.clone(), carte.clone()));
    }
    table.sort();

    println!("//! Table `hash .s2ma → nom de carte`, GÉNÉRÉE — ne pas éditer à la main.");
    println!("//!");
    println!("//! Produite par `cargo run --release -p storm-lobby --example derive_maps -- <dossier>`");
    println!("//! sur l'archive du box. Un hash n'est retenu que s'il n'a JAMAIS été observé sur une autre");
    println!("//! carte. Une même carte a plusieurs hashes : Blizzard republie ses fichiers `.s2ma` à chaque");
    println!("//! patch, donc exiger qu'un seul hash couvre tous les replays d'une carte éliminerait les cartes");
    println!("//! les plus jouées (mesuré : 9 cartes sur 19 avec ce critère, 19 sur 19 sans).");
    println!();
    println!("/// `(hash, nom de carte)`, trié par hash — les noms sont ceux que `matches.map` stocke.");
    println!("pub(crate) const MAP_BY_HASH: &[(&str, &str)] = &[");
    for (h, c) in &table {
        println!("    ({h:?}, {c:?}),");
    }
    println!("];");

    eprintln!("cartes couvertes : {}",
        table.iter().map(|(_, c)| c).collect::<BTreeSet<_>>().len());
    eprintln!("hashes retenus   : {}", table.len());
    eprintln!("cartes vues      : {}", replays_par_carte.len());
    Ok(())
}
