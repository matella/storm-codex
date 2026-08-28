//! Parité du parser autonome contre le parse complet, sur un dossier de replays.
//!
//! Usage : cargo run --release -p storm-lobby --example parity -- <dossier> [max]
//!
//! Deux mesures indépendantes, à ne pas confondre :
//!   - `battletags exacts` : le parser retrouve exactement les 10 BattleTags de la partie.
//!   - `équipes exactes`   : parmi ceux-là, la déduction par l'ordre (5+5) correspond à la vérité.
//!
//!   La première décide du go/no-go ; la seconde décide si le companion affiche deux équipes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Default)]
struct Tally {
    total: usize,
    stats_rejete: usize,
    lobby_err: usize,
    battletags_exacts: usize,
    equipes_exactes: usize,
    equipes_evaluables: usize,
    divergents: Vec<(String, String)>,
    par_build: BTreeMap<u32, (usize, usize)>, // build → (évalués, battletags exacts)
}

/// `{battletag: équipe}` d'après le parse complet, ou `None` si le replay est rejeté.
fn verite(path: &Path) -> Option<BTreeMap<String, i64>> {
    let filename = path.file_name()?.to_str()?;
    let out = storm_stats::process_replay(path, filename);
    if out.status != 1 {
        return None;
    }
    let json = out.to_json();
    Some(
        json["players"]
            .as_object()?
            .values()
            .filter_map(|p| {
                let name = p["name"].as_str()?;
                let tag = p["tag"].as_i64()?;
                let team = p["team"].as_i64()?;
                Some((format!("{name}#{tag}"), team))
            })
            .collect(),
    )
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let dir: PathBuf = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("usage: parity <dossier> [max]"))?
        .into();
    let max: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(usize::MAX);

    let mut t = Tally::default();
    for entry in std::fs::read_dir(&dir)? {
        if t.total >= max {
            break;
        }
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("StormReplay") {
            continue;
        }
        t.total += 1;
        let label = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();

        let Ok(replay) = storm_replay::Replay::open(&path) else {
            t.divergents.push((label, "ouverture impossible".into()));
            continue;
        };
        let build = replay.header.base_build;
        let Ok(blob) = replay.battlelobby_raw() else {
            t.divergents.push((label, "blob absent".into()));
            continue;
        };
        let Some(attendu) = verite(&path) else {
            t.stats_rejete += 1;
            continue;
        };
        if attendu.len() != 10 {
            t.stats_rejete += 1;
            continue;
        }

        let compteur = t.par_build.entry(build).or_default();
        compteur.0 += 1;

        match storm_lobby::parse(&blob) {
            Err(e) => {
                t.lobby_err += 1;
                t.divergents.push((label, format!("parse: {e}")));
            }
            Ok(lobby) => {
                let tags_ok = lobby.players.len() == attendu.len()
                    && lobby.players.iter().all(|p| attendu.contains_key(&p.battletag()));
                if !tags_ok {
                    t.divergents
                        .push((label, format!("{} battletags décodés", lobby.players.len())));
                    continue;
                }
                t.battletags_exacts += 1;
                compteur.1 += 1;

                if lobby.players.iter().any(|p| p.team.is_some()) {
                    t.equipes_evaluables += 1;
                    if lobby.players.iter().all(|p| match p.team {
                        None => true,
                        Some(team) => attendu.get(&p.battletag()) == Some(&i64::from(team)),
                    }) {
                        t.equipes_exactes += 1;
                    } else {
                        t.divergents.push((label, "équipes divergentes".into()));
                    }
                }
            }
        }
    }

    let base = t.total - t.stats_rejete;
    let pct = |n: usize, d: usize| if d == 0 { 0.0 } else { 100.0 * n as f64 / d as f64 };
    println!("replays vus          : {}", t.total);
    println!("écartés (stats)      : {}", t.stats_rejete);
    println!("base de comparaison  : {base}");
    println!(
        "battletags exacts    : {} ({:.2} %)",
        t.battletags_exacts,
        pct(t.battletags_exacts, base)
    );
    println!("erreurs de parse     : {}", t.lobby_err);
    println!(
        "équipes exactes      : {} / {} évaluables ({:.2} %)",
        t.equipes_exactes,
        t.equipes_evaluables,
        pct(t.equipes_exactes, t.equipes_evaluables)
    );
    println!("\npar build (build: battletags exacts/évalués)");
    for (build, (evalues, exacts)) in &t.par_build {
        println!("  {build}: {exacts}/{evalues}");
    }
    println!("\n20 premières divergences");
    for (f, why) in t.divergents.iter().take(20) {
        println!("  {f} — {why}");
    }
    Ok(())
}
