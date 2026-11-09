//! `parse` consumes potentially malformed binary (new Blizzard build, file read
//! while the game is writing it). It must ALWAYS return, never panic.
//!
//! In the last two tests, **the absence of panic IS the assertion**: the test harness
//! fails by itself if `parse` panics. The result is deliberately ignored, because no return
//! value is "correct" on a corrupted input — only the absence of panic is.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;

fn blob_reel() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/storm-stats/tests/data/silver-city-aram.StormReplay");
    let replay = storm_replay::Replay::open(&path).expect("ouverture replay");
    replay.battlelobby_raw().expect("stream battlelobby")
}

#[test]
fn entree_vide_est_une_erreur() {
    assert!(storm_lobby::parse(&[]).is_err(), "empty blob accepted");
}

#[test]
fn entree_aleatoire_est_une_erreur_pas_une_panique() {
    // Deterministic generator (xorshift): no dependency, reproducible result.
    let mut state: u32 = 0x1234_5678;
    let mut bytes = Vec::with_capacity(4096);
    for _ in 0..4096 {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        bytes.push((state & 0xff) as u8);
    }
    assert!(
        storm_lobby::parse(&bytes).is_err(),
        "random noise was accepted as a lobby"
    );
}

#[test]
fn blob_tronque_a_toute_longueur_ne_panique_jamais() {
    let blob = blob_reel();
    // A file read while being written is a prefix of the complete file: we exercise exactly
    // that case, at every truncation point (step of 97 to stay fast).
    for n in (0..blob.len()).step_by(97) {
        let _ = storm_lobby::parse(&blob[..n]);
    }
}

#[test]
fn blob_corrompu_en_son_milieu_ne_panique_jamais() {
    let mut blob = blob_reel();
    let milieu = blob.len() / 2;
    let fin = (milieu + 64).min(blob.len());
    for b in &mut blob[milieu..fin] {
        *b = 0xff;
    }
    let _ = storm_lobby::parse(&blob);
}
