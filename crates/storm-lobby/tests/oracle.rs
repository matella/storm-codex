//! Parity oracle: from the lobby blob alone, the parser must find the players that the
//! full parse of the replay identifies — by BattleTag, the only identity the format exposes.
//!
//! Each `.StormReplay` carries the same blob as the one written live by the game: each replay is
//! therefore both a sample and its own answer key.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use storm_lobby::LobbyPlayer;

fn data(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

/// The replays committed in the workspace.
fn replays() -> Vec<PathBuf> {
    [
        "crates/storm-replay/tests/data/2024-10-07 22.29.55 Industrial District.StormReplay",
        "crates/storm-replay/tests/data/2026-05-27 22.37.45 Industrial District.StormReplay",
        "crates/storm-replay/tests/data/2026-06-09 20.35.02 Industrial District.StormReplay",
        "crates/storm-stats/tests/data/silver-city-aram.StormReplay",
    ]
    .iter()
    .map(|p| data(p))
    .collect()
}

/// Reference truth: `{battletag: team}` from the full parse of the replay.
/// The players whose discriminator the full parse did not resolve are discarded: the oracle cannot
/// judge what its own reference ignores.
fn truth(path: &Path) -> BTreeMap<String, i64> {
    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .expect("file name");
    let out = storm_stats::process_replay(path, filename);
    assert_eq!(out.status, 1, "{filename}: full parse rejected");
    let json = out.to_json();
    json["players"]
        .as_object()
        .expect("players")
        .values()
        .filter_map(|p| {
            let name = p["name"].as_str()?;
            let tag = p["tag"].as_i64()?;
            let team = p["team"].as_i64()?;
            Some((format!("{name}#{tag}"), team))
        })
        .collect()
}

fn lobby_of(path: &Path) -> storm_lobby::Lobby {
    let replay = storm_replay::Replay::open(path).expect("ouverture replay");
    let blob = replay.battlelobby_raw().expect("stream battlelobby");
    storm_lobby::parse(&blob).expect("parse lobby")
}

#[test]
fn parse_retrouve_les_battletags_du_replay() {
    for path in replays() {
        let label = path.file_name().and_then(|s| s.to_str()).expect("nom");
        let expected = truth(&path);
        assert_eq!(expected.len(), 10, "{label}: incomplete reference");

        let decoded: BTreeMap<String, Option<u8>> = lobby_of(&path)
            .players
            .iter()
            .map(|p| (p.battletag(), p.team))
            .collect();

        let attendus: Vec<&String> = expected.keys().collect();
        let obtenus: Vec<&String> = decoded.keys().collect();
        assert_eq!(obtenus, attendus, "{label}: divergent BattleTags");
    }
}

/// The format carries no team field: it is deduced from the order (first 5 / last 5).
/// This hypothesis was only observed on 2 samples in task 1 — this test checks it on the
/// committed corpus, and the parity harness (task 5) checks it on the whole archive.
#[test]
fn les_equipes_deduites_correspondent_au_replay() {
    for path in replays() {
        let label = path.file_name().and_then(|s| s.to_str()).expect("nom");
        let expected = truth(&path);
        for p in &lobby_of(&path).players {
            let Some(team) = p.team else { continue };
            let attendu = expected
                .get(&p.battletag())
                .unwrap_or_else(|| panic!("{label}: unknown BattleTag \"{}\"", p.battletag()));
            assert_eq!(
                i64::from(team),
                *attendu,
                "{label}: divergent team for {}",
                p.battletag()
            );
        }
    }
}

/// The blob contains Battle.net cache paths to `.s2ma` files (the maps),
/// identified by a hash of 32 hexadecimal characters. They are the only map lead of the
/// format (format report, Q5).
#[test]
fn les_hashes_de_carte_sont_extraits() {
    let path = data("crates/storm-stats/tests/data/silver-city-aram.StormReplay");
    let replay = storm_replay::Replay::open(&path).expect("ouverture replay");
    let blob = replay.battlelobby_raw().expect("stream battlelobby");
    let hashes = storm_lobby::map_hashes(&blob);
    assert!(
        !hashes.is_empty(),
        "no .s2ma hash extracted — the format report documents 9"
    );
    assert!(
        hashes
            .iter()
            .all(|h| h.len() == 32 && h.chars().all(|c| c.is_ascii_hexdigit())),
        "malformed hash: {hashes:?}"
    );
}

/// The map deduced from the hashes must match the one of the full parse, when it is deduced.
/// A `None` map is acceptable (hash unknown to the table); a WRONG map is not.
#[test]
fn la_carte_deduite_ne_ment_jamais() {
    for path in replays() {
        let label = path.file_name().and_then(|s| s.to_str()).expect("nom");
        let filename = label;
        let out = storm_stats::process_replay(&path, filename);
        assert_eq!(out.status, 1, "{label}: full parse rejected");
        let json = out.to_json();
        let attendue = json["match"]["map"]
            .as_str()
            .unwrap_or_default()
            .to_string();

        if let Some(deduite) = lobby_of(&path).map {
            assert_eq!(deduite, attendue, "{label}: wrongly deduced map");
        }
    }
}

/// Regression. A Cyrillic BattleTag is present in this replay as multi-byte UTF-8; `strings`
/// does not see it. A parser that assumed ASCII would silently lose one player in ten
/// in European games — without failing, which is worse. See the format report, Q1.
#[test]
fn battletag_non_ascii_est_decode() {
    let path =
        data("crates/storm-replay/tests/data/2026-06-09 20.35.02 Industrial District.StormReplay");
    let lobby = lobby_of(&path);
    assert!(
        lobby
            .players
            .iter()
            .any(|p| p.name == "ЛовкийЭльф" && p.discriminant == "215346"),
        "Cyrillic BattleTag missing — decoded: {:?}",
        lobby
            .players
            .iter()
            .map(LobbyPlayer::battletag)
            .collect::<Vec<_>>()
    );
}
