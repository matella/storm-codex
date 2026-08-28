//! Parité du parser autonome contre le parse complet, sur un dossier de replays.
//!
//! Usage : cargo run --release -p storm-lobby --example parity -- <dossier> [max]
//!
//! Deux mesures indépendantes, à ne pas confondre :
//!   - `battletags exacts` : le parser retrouve exactement les 10 BattleTags de la partie.
//!   - `équipes exactes`   : parmi ceux-là, la déduction par l'ordre (5+5) correspond à la vérité.
//!
//!   La première décide du go/no-go ; la seconde décide si le companion affiche deux équipes.
//!
//! Ce harnais produit lui-même toutes les preuves du rapport de parité
//! (`docs/research/2026-08-27-lobby-parity.md`) : ventilation par mode de jeu, distinction
//! sur-capture / sous-capture (comptée sur l'archive entière, pas sur un échantillon), et le détail
//! nommé des premières divergences. Rien dans le rapport ne doit venir d'un script non committé.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

/// Sentinelle pour un mode absent du JSON de vérité — distincte de `-1` (Personnalisée), qui est un
/// mode réel.
const MODE_INCONNU: i64 = i64::MIN;

#[derive(Default)]
struct ModeTally {
    evalues: usize,
    battletags_exacts: usize,
    equipes_evaluables: usize,
    equipes_exactes: usize,
}

struct Divergence {
    label: String,
    reason: String,
    en_trop: Vec<String>,
    manquants: Vec<String>,
}

#[derive(Default)]
struct Tally {
    total: usize,
    /// Replays qui sortent avant même que la vérité soit calculable : `Replay::open` échoue, ou
    /// `battlelobby_raw` est absent. Doivent être retranchés de la base, comme `stats_rejete` —
    /// sinon la base est gonflée et le taux publié sous-estime le parser.
    non_evaluables: usize,
    /// Rejetés par `storm-stats`, ou vérité incomplète (≠ 10 joueurs résolus).
    stats_rejete: usize,
    lobby_err: usize,
    battletags_exacts: usize,
    equipes_exactes: usize,
    equipes_evaluables: usize,
    /// Équipes fausses mais les deux camps simplement PERMUTÉS : l'ordre porte bien
    /// l'information d'équipe, il manque seulement de savoir quel côté est lequel. Réparable
    /// d'un clic côté produit.
    equipes_inversion: usize,
    /// Équipes fausses sans être une permutation : l'ordre du lobby ne porte aucune
    /// information d'équipe. Rien à réparer — l'information est absente.
    equipes_disperse: usize,
    /// Plus de 10 décodés, mais les 10 vrais joueurs tous présents : des occupants en plus
    /// (observateurs), jamais un joueur manquant.
    sur_capture: usize,
    /// Tout le reste : un vrai joueur manque, ou le décodage est faux d'une autre façon.
    sous_capture_ou_faux: usize,
    par_build: BTreeMap<u32, (usize, usize)>, // build → (évalués, battletags exacts)
    par_mode: BTreeMap<i64, ModeTally>,
    divergents: Vec<Divergence>,
}

/// `{battletag: équipe}` d'après le parse complet, et le mode de la partie, ou `None` si le replay
/// est rejeté par `storm-stats`.
fn verite(path: &Path) -> Option<(i64, BTreeMap<String, i64>)> {
    let filename = path.file_name()?.to_str()?;
    let out = storm_stats::process_replay(path, filename);
    if out.status != 1 {
        return None;
    }
    let json = out.to_json();
    let mode = json["match"]["mode"].as_i64().unwrap_or(MODE_INCONNU);
    let players = json["players"]
        .as_object()?
        .values()
        .filter_map(|p| {
            let name = p["name"].as_str()?;
            let tag = p["tag"].as_i64()?;
            let team = p["team"].as_i64()?;
            Some((format!("{name}#{tag}"), team))
        })
        .collect();
    Some((mode, players))
}

/// Nom lisible du mode, pour l'affichage du tableau.
fn nom_mode(mode: i64) -> String {
    match mode {
        50091 => "50091 — Storm League".into(),
        50101 => "50101 — ARAM".into(),
        50001 => "50001 — Quick Match".into(),
        -1 => "-1 — Personnalisée".into(),
        MODE_INCONNU => "mode inconnu (absent du JSON)".into(),
        other => format!("{other} — inconnu"),
    }
}

/// Classe une divergence de BattleTags : sur-capture (les 10 vrais sont tous là, plus des occupants
/// en trop) ou sous-capture/décodage faux (tout le reste — un vrai joueur manque).
fn classer(decoded: &[storm_lobby::LobbyPlayer], attendu: &BTreeMap<String, i64>) -> (bool, Vec<String>, Vec<String>) {
    let decoded_tags: Vec<String> = decoded.iter().map(storm_lobby::LobbyPlayer::battletag).collect();
    let decoded_set: HashSet<&String> = decoded_tags.iter().collect();
    let en_trop: Vec<String> = decoded_tags
        .iter()
        .filter(|bt| !attendu.contains_key(*bt))
        .cloned()
        .collect();
    let manquants: Vec<String> = attendu
        .keys()
        .filter(|bt| !decoded_set.contains(*bt))
        .cloned()
        .collect();
    let sur_capture = manquants.is_empty() && decoded.len() > attendu.len();
    (sur_capture, en_trop, manquants)
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let dir: PathBuf = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("usage: parity <dossier> [max]"))?
        .into();
    // Un `max` mal tapé doit faire échouer l'outil, pas retomber silencieusement sur "tout parcourir"
    // — sur un harnais de mesure, une valeur illisible avalée en silence fausserait la base sans
    // qu'on s'en aperçoive.
    let max: usize = match args.next() {
        Some(s) => s
            .parse()
            .map_err(|e| anyhow::anyhow!("max invalide « {s} » : {e}"))?,
        None => usize::MAX,
    };

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
            t.non_evaluables += 1;
            continue;
        };
        let build = replay.header.base_build;
        let Ok(blob) = replay.battlelobby_raw() else {
            t.non_evaluables += 1;
            continue;
        };
        let Some((mode, attendu)) = verite(&path) else {
            t.stats_rejete += 1;
            continue;
        };
        if attendu.len() != 10 {
            t.stats_rejete += 1;
            continue;
        }

        let build_compteur = t.par_build.entry(build).or_default();
        build_compteur.0 += 1;
        let mode_tally = t.par_mode.entry(mode).or_default();
        mode_tally.evalues += 1;

        match storm_lobby::parse(&blob) {
            Err(e) => {
                t.lobby_err += 1;
                t.sous_capture_ou_faux += 1;
                t.divergents.push(Divergence {
                    label,
                    reason: format!("parse: {e}"),
                    en_trop: Vec::new(),
                    manquants: attendu.keys().cloned().collect(),
                });
            }
            Ok(lobby) => {
                let tags_ok = lobby.players.len() == attendu.len()
                    && lobby.players.iter().all(|p| attendu.contains_key(&p.battletag()));
                if !tags_ok {
                    let (sur_capture, en_trop, manquants) = classer(&lobby.players, &attendu);
                    if sur_capture {
                        t.sur_capture += 1;
                    } else {
                        t.sous_capture_ou_faux += 1;
                    }
                    t.divergents.push(Divergence {
                        label,
                        reason: format!("{} battletags décodés", lobby.players.len()),
                        en_trop,
                        manquants,
                    });
                    continue;
                }
                t.battletags_exacts += 1;
                let mode_tally = t.par_mode.entry(mode).or_default();
                mode_tally.battletags_exacts += 1;
                build_compteur.1 += 1;

                if lobby.players.iter().any(|p| p.team.is_some()) {
                    t.equipes_evaluables += 1;
                    mode_tally.equipes_evaluables += 1;
                    if lobby.players.iter().all(|p| match p.team {
                        None => true,
                        Some(team) => attendu.get(&p.battletag()) == Some(&i64::from(team)),
                    }) {
                        t.equipes_exactes += 1;
                        mode_tally.equipes_exactes += 1;
                    } else {
                        // Permutation complète : chaque joueur est dans l'équipe opposée à celle
                        // qu'on lui a assignée. C'est la seule forme d'erreur d'équipe qu'un
                        // produit puisse réparer d'un clic — d'où le comptage séparé.
                        let inversion = lobby.players.iter().all(|p| match p.team {
                            None => true,
                            Some(team) => {
                                attendu.get(&p.battletag()) == Some(&i64::from(1 - team))
                            }
                        });
                        if inversion {
                            t.equipes_inversion += 1;
                        } else {
                            t.equipes_disperse += 1;
                        }
                        t.divergents.push(Divergence {
                            label,
                            reason: "équipes divergentes".into(),
                            en_trop: Vec::new(),
                            manquants: Vec::new(),
                        });
                    }
                }
            }
        }
    }

    // La base doit être dérivée de ce qui a réellement été évalué : ni les non-évaluables (échec
    // d'ouverture / blob absent), ni les rejetés par storm-stats, ne doivent y rester silencieusement.
    let base = t.total - t.non_evaluables - t.stats_rejete;
    let somme_modes: usize = t.par_mode.values().map(|m| m.evalues).sum();
    assert_eq!(
        base, somme_modes,
        "incohérence interne : la base ({base}) ne correspond pas à la somme des évalués par mode ({somme_modes})"
    );

    let pct = |n: usize, d: usize| if d == 0 { 0.0 } else { 100.0 * n as f64 / d as f64 };

    println!("replays vus          : {}", t.total);
    println!("non-évaluables       : {} (ouverture/blob)", t.non_evaluables);
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
    let fausses = t.equipes_inversion + t.equipes_disperse;
    println!(
        "  dont inversion franche (camps permutés)   : {} ({:.2} % des fausses)",
        t.equipes_inversion,
        pct(t.equipes_inversion, fausses)
    );
    println!(
        "  dont ordre sans information d'équipe      : {} ({:.2} % des fausses)",
        t.equipes_disperse,
        pct(t.equipes_disperse, fausses)
    );

    println!("\nmode d'échec (sur la base entière, {base} replays)");
    println!(
        "  sur-capture (10 vrais + occupants en trop) : {} ({:.2} %)",
        t.sur_capture,
        pct(t.sur_capture, base)
    );
    println!(
        "  sous-capture ou décodage faux               : {} ({:.2} %)",
        t.sous_capture_ou_faux,
        pct(t.sous_capture_ou_faux, base)
    );

    println!("\npar mode de jeu");
    println!(
        "  {:<32} {:>8} {:>22} {:>22}",
        "mode", "évalués", "battletags exacts", "équipes exactes"
    );
    for (mode, m) in &t.par_mode {
        println!(
            "  {:<32} {:>8} {:>14} ({:>5.2} %) {:>14} ({:>5.2} %)",
            nom_mode(*mode),
            m.evalues,
            m.battletags_exacts,
            pct(m.battletags_exacts, m.evalues),
            m.equipes_exactes,
            pct(m.equipes_exactes, m.equipes_evaluables)
        );
    }

    println!("\npar build (build: battletags exacts/évalués)");
    for (build, (evalues, exacts)) in &t.par_build {
        println!("  {build}: {exacts}/{evalues}");
    }

    println!("\n20 premières divergences (battletags en trop / manquants nommés)");
    for d in t.divergents.iter().take(20) {
        println!("  {} — {}", d.label, d.reason);
        for bt in &d.en_trop {
            println!("      EN TROP    -> {bt}");
        }
        for bt in &d.manquants {
            println!("      MANQUANT   -> {bt}");
        }
    }

    Ok(())
}
