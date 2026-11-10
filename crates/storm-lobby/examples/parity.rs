//! Parity of the standalone parser against the full parse, on a folder of replays.
//!
//! Usage: cargo run --release -p storm-lobby --example parity -- <folder> [max]
//!
//! Two independent measurements, not to be confused:
//!   - `exact battletags`: the parser finds exactly the 10 BattleTags of the game.
//!   - `exact teams`     : among those, the deduction from the order (5+5) matches the truth.
//!
//!   The first one decides the go/no-go; the second decides whether the companion shows two teams.
//!
//! This harness itself produces all the evidence of the parity report
//! (`docs/research/2026-08-27-lobby-parity.md`): breakdown per game mode, distinction
//! over-capture / under-capture (counted on the whole archive, not on a sample), and the named
//! detail of the first divergences. Nothing in the report may come from an uncommitted script.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

/// Sentinel for a mode missing from the truth JSON — distinct from `-1` (Custom), which is a
/// real mode.
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
    /// Replays that drop out before the truth can even be computed: `Replay::open` fails, or
    /// `battlelobby_raw` is missing. Must be subtracted from the base, like `stats_rejete` —
    /// otherwise the base is inflated and the published rate underestimates the parser.
    non_evaluables: usize,
    /// Rejected by `storm-stats`, or incomplete truth (≠ 10 resolved players).
    stats_rejete: usize,
    lobby_err: usize,
    battletags_exacts: usize,
    equipes_exactes: usize,
    equipes_evaluables: usize,
    /// Wrong teams but the two sides simply SWAPPED: the order does carry the
    /// team information, only which side is which is missing. Fixable
    /// in one click on the product side.
    equipes_inversion: usize,
    /// Wrong teams without being a swap: the lobby order carries no
    /// team information. Nothing to fix — the information is absent.
    equipes_disperse: usize,
    /// More than 10 decoded, but the 10 real players all present: extra occupants
    /// (observers), never a missing player.
    sur_capture: usize,
    /// Everything else: a real player is missing, or the decoding is wrong in another way.
    sous_capture_ou_faux: usize,
    par_build: BTreeMap<u32, (usize, usize)>, // build → (evaluated, exact battletags)
    par_mode: BTreeMap<i64, ModeTally>,
    divergents: Vec<Divergence>,
}

/// `{battletag: team}` from the full parse, and the game mode, or `None` if the replay
/// is rejected by `storm-stats`.
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

/// Readable mode name, for the table display.
fn nom_mode(mode: i64) -> String {
    match mode {
        50091 => "50091 — Storm League".into(),
        50101 => "50101 — ARAM".into(),
        50001 => "50001 — Quick Match".into(),
        -1 => "-1 — Custom".into(),
        MODE_INCONNU => "unknown mode (missing from the JSON)".into(),
        other => format!("{other} — unknown"),
    }
}

/// Classifies a BattleTag divergence: over-capture (the 10 real ones are all there, plus extra
/// occupants) or under-capture/wrong decoding (everything else — a real player is missing).
fn classer(
    decoded: &[storm_lobby::LobbyPlayer],
    attendu: &BTreeMap<String, i64>,
) -> (bool, Vec<String>, Vec<String>) {
    let decoded_tags: Vec<String> = decoded
        .iter()
        .map(storm_lobby::LobbyPlayer::battletag)
        .collect();
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
    // A mistyped `max` must make the tool fail, not silently fall back to "walk everything"
    // — on a measurement harness, an unreadable value silently swallowed would skew the base without
    // anyone noticing.
    let max: usize = match args.next() {
        Some(s) => s
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid max \"{s}\": {e}"))?,
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
                    && lobby
                        .players
                        .iter()
                        .all(|p| attendu.contains_key(&p.battletag()));
                if !tags_ok {
                    let (sur_capture, en_trop, manquants) = classer(&lobby.players, &attendu);
                    if sur_capture {
                        t.sur_capture += 1;
                    } else {
                        t.sous_capture_ou_faux += 1;
                    }
                    t.divergents.push(Divergence {
                        label,
                        reason: format!("{} battletags decoded", lobby.players.len()),
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
                        // Full swap: each player is in the team opposite to the one
                        // we assigned. It is the only form of team error that a
                        // product can fix in one click — hence the separate count.
                        let inversion = lobby.players.iter().all(|p| match p.team {
                            None => true,
                            Some(team) => attendu.get(&p.battletag()) == Some(&i64::from(1 - team)),
                        });
                        if inversion {
                            t.equipes_inversion += 1;
                        } else {
                            t.equipes_disperse += 1;
                        }
                        t.divergents.push(Divergence {
                            label,
                            reason: "divergent teams".into(),
                            en_trop: Vec::new(),
                            manquants: Vec::new(),
                        });
                    }
                }
            }
        }
    }

    // The base must be derived from what was really evaluated: neither the non-evaluable ones (open
    // failure / missing blob) nor those rejected by storm-stats may silently stay in it.
    let base = t.total - t.non_evaluables - t.stats_rejete;
    let somme_modes: usize = t.par_mode.values().map(|m| m.evalues).sum();
    assert_eq!(
        base, somme_modes,
        "internal inconsistency: the base ({base}) does not match the sum evaluated per mode ({somme_modes})"
    );

    let pct = |n: usize, d: usize| {
        if d == 0 {
            0.0
        } else {
            100.0 * n as f64 / d as f64
        }
    };

    println!("replays seen         : {}", t.total);
    println!("not evaluable        : {} (open/blob)", t.non_evaluables);
    println!("rejected (stats)     : {}", t.stats_rejete);
    println!("comparison base      : {base}");
    println!(
        "exact battletags     : {} ({:.2} %)",
        t.battletags_exacts,
        pct(t.battletags_exacts, base)
    );
    println!("parse errors         : {}", t.lobby_err);
    println!(
        "exact teams          : {} / {} evaluable ({:.2} %)",
        t.equipes_exactes,
        t.equipes_evaluables,
        pct(t.equipes_exactes, t.equipes_evaluables)
    );
    let fausses = t.equipes_inversion + t.equipes_disperse;
    println!(
        "  of which clean swap (sides swapped)       : {} ({:.2} % of wrong ones)",
        t.equipes_inversion,
        pct(t.equipes_inversion, fausses)
    );
    println!(
        "  of which order without team information  : {} ({:.2} % of wrong ones)",
        t.equipes_disperse,
        pct(t.equipes_disperse, fausses)
    );

    println!("\nfailure mode (over the whole base, {base} replays)");
    println!(
        "  over-capture (10 real + extra occupants)   : {} ({:.2} %)",
        t.sur_capture,
        pct(t.sur_capture, base)
    );
    println!(
        "  under-capture or wrong decoding            : {} ({:.2} %)",
        t.sous_capture_ou_faux,
        pct(t.sous_capture_ou_faux, base)
    );

    println!("\nper game mode");
    println!(
        "  {:<32} {:>8} {:>22} {:>22}",
        "mode", "evaluated", "exact battletags", "exact teams"
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

    println!("\nper build (build: exact battletags/evaluated)");
    for (build, (evalues, exacts)) in &t.par_build {
        println!("  {build}: {exacts}/{evalues}");
    }

    println!("\nfirst 20 divergences (extra / missing battletags named)");
    for d in t.divergents.iter().take(20) {
        println!("  {} — {}", d.label, d.reason);
        for bt in &d.en_trop {
            println!("      EXTRA      -> {bt}");
        }
        for bt in &d.manquants {
            println!("      MISSING    -> {bt}");
        }
    }

    Ok(())
}
