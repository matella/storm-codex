//! `StreamDecoder` : décoder un stream par morceaux doit rendre exactement les mêmes événements que
//! le décodage du replay entier, quelle que soit la découpe — c'est ce que le feed live reçoit
//! (blocs de 4 Ko écrits par le jeu, cf. docs/research/2026-10-24-tempwritereplay-live.md).

#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use storm_replay::{Replay, StreamDecoder, StreamKind, Value};

fn mini_corpus() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "StormReplay"))
        .collect();
    files.sort();
    assert!(!files.is_empty());
    files
}

/// Découpe déterministe pseudo-aléatoire (LCG) : tailles de 1 à 9 000 octets.
fn random_sizes(seed: u64) -> impl FnMut() -> usize {
    let mut x = seed;
    move || {
        x = x
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        1 + (x >> 33) as usize % 9_000
    }
}

fn feed_all(
    kind: StreamKind,
    build: u32,
    raw: &[u8],
    mut next_size: impl FnMut() -> usize,
) -> (Vec<Value>, StreamDecoder) {
    let mut dec = StreamDecoder::new(kind, Some(build)).unwrap();
    let mut out = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        let end = (i + next_size()).min(raw.len());
        out.extend(dec.feed(&raw[i..end]).unwrap());
        i = end;
    }
    (out, dec)
}

#[test]
fn chunked_decoding_equals_whole_replay() {
    for path in mini_corpus() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let replay = Replay::open(&path).unwrap();
        let build = replay.header.base_build;
        let cases: [(StreamKind, Vec<u8>, Vec<Value>); 2] = [
            (
                StreamKind::Tracker,
                replay.tracker_events_raw().unwrap(),
                replay.tracker_events().unwrap(),
            ),
            (
                StreamKind::Game,
                replay.game_events_raw().unwrap(),
                replay.game_events().unwrap(),
            ),
        ];
        for (kind, raw, whole) in &cases {
            for (label, sizes) in [
                ("4 Ko", Box::new(|| 4096_usize) as Box<dyn FnMut() -> usize>),
                ("aléatoire", Box::new(random_sizes(raw.len() as u64))),
            ] {
                let (events, dec) = feed_all(*kind, build, raw, sizes);
                assert_eq!(events.len(), whole.len(), "{name} {kind:?} {label}: nombre");
                assert!(events == *whole, "{name} {kind:?} {label}: contenu");
                assert_eq!(dec.pending(), 0, "{name} {kind:?} {label}: queue non vide");
                assert_eq!(dec.consumed(), raw.len() as u64, "{name} {kind:?} {label}");
            }
        }
    }
}

#[test]
fn byte_by_byte_on_a_prefix() {
    // 1 octet à la fois coûte un redécodage par octet : on se limite aux 20 premiers Ko.
    let replay = Replay::open(&mini_corpus()[0]).unwrap();
    let build = replay.header.base_build;
    for kind in [StreamKind::Tracker, StreamKind::Game] {
        let raw = match kind {
            StreamKind::Tracker => replay.tracker_events_raw().unwrap(),
            StreamKind::Game => replay.game_events_raw().unwrap(),
        };
        let prefix = &raw[..raw.len().min(20_000)];
        let (by_byte, d1) = feed_all(kind, build, prefix, || 1);
        let (at_once, d2) = feed_all(kind, build, prefix, || usize::MAX);
        assert!(by_byte == at_once, "{kind:?}");
        assert_eq!(d1.consumed(), d2.consumed());
        assert_eq!(d1.gameloop(), d2.gameloop());
        assert!(d1.pending() < 20_000);
    }
}

#[test]
fn random_bytes_never_panic() {
    let mut next = random_sizes(42);
    for kind in [StreamKind::Tracker, StreamKind::Game] {
        for round in 0..200 {
            let junk: Vec<u8> = (0..(next() % 512)).map(|_| next() as u8).collect();
            let mut dec = StreamDecoder::new(kind, None).unwrap();
            // Erreur ou événements : les deux sont acceptables, une panique ne l'est pas.
            let _ = dec.feed(&junk);
            let _ = dec.feed(&junk);
            assert!(dec.consumed() <= 2 * junk.len() as u64, "{kind:?} #{round}");
        }
    }
}

#[test]
fn unknown_build_reports_fallback() {
    let dec = StreamDecoder::new(StreamKind::Tracker, Some(1)).unwrap();
    assert_eq!(
        dec.protocol_fallback(),
        Some((1, storm_replay::LATEST_BUILD))
    );
    let dec = StreamDecoder::new(StreamKind::Game, None).unwrap();
    assert_eq!(dec.protocol_fallback(), None);
}
