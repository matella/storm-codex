//! Simulateur du feed live : rejoue un `.StormReplay` comme si la partie se déroulait, en postant
//! au serveur ce que le watcher `client-rs` postera depuis `TempWriteReplayP1` — le battlelobby,
//! puis `tracker.events` et `game.events` par blocs de 4 Ko, dans l'ordre des game loops, au rythme
//! du jeu multiplié par `--speed`, et enfin la fin de partie.
//!
//! ```text
//! cargo run --release -p storm-codex-server --example live_sim -- <replay> \
//!     [--url http://127.0.0.1:8089] [--token <jeton d'upload> | env STORM_TOKEN] [--speed 8]
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::{Duration, Instant};
use storm_replay::{Replay, StreamDecoder, StreamKind};

const BLOCK: usize = 4096;
const LOOPS_PER_SEC: f64 = 16.0;

struct Chunk {
    kind: StreamKind,
    offset: usize,
    bytes: Vec<u8>,
    /// Le jeu écrit un bloc une fois son contenu produit : disponible à ce gameloop.
    ready_loop: i64,
}

fn arg(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn schedule(kind: StreamKind, raw: &[u8], build: u32) -> Vec<Chunk> {
    let mut dec = StreamDecoder::new(kind, Some(build)).unwrap();
    raw.chunks(BLOCK)
        .enumerate()
        .map(|(i, block)| {
            dec.feed(block).unwrap();
            Chunk {
                kind,
                offset: i * BLOCK,
                bytes: block.to_vec(),
                ready_loop: dec.gameloop(),
            }
        })
        .collect()
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage : live_sim <replay> [--url] [--token] [--speed]");
    let url = arg("--url").unwrap_or_else(|| "http://127.0.0.1:8089".into());
    let token = arg("--token")
        .or_else(|| std::env::var("STORM_TOKEN").ok())
        .expect("--token ou STORM_TOKEN requis (jeton d'upload)");
    let speed: f64 = arg("--speed").and_then(|s| s.parse().ok()).unwrap_or(8.0);

    let bytes = std::fs::read(&path).unwrap();
    let replay = Replay::from_bytes(bytes).unwrap();
    let build = replay.header.base_build;
    // Contrat de `X-Session` : SHA-256 du battlelobby, que le serveur rapproche du lobby courant
    // (`lobby_live.content_hash`) pour en tirer la carte et l'opérateur.
    let lobby = replay.battlelobby_raw().unwrap();
    let session = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(&lobby));
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into();
    let auth = format!("Bearer {token}");

    // 0. Repart d'une session vierge (rejouer le même replay réutiliserait son id).
    let r = agent.delete(format!("{url}/api/live")).call().unwrap();
    println!("reset → {}", r.status());

    // 1. Le battlelobby, comme pendant l'écran de chargement.
    let mut r = agent
        .post(format!("{url}/api/lobby"))
        .header("Authorization", &auth)
        .send(&lobby[..])
        .unwrap();
    println!(
        "lobby → {} {}",
        r.status(),
        r.body_mut().read_to_string().unwrap_or_default()
    );

    // 2. Les blocs des deux streams, dans l'ordre où le jeu les écrirait.
    let mut chunks = schedule(
        StreamKind::Tracker,
        &replay.tracker_events_raw().unwrap(),
        build,
    );
    chunks.extend(schedule(
        StreamKind::Game,
        &replay.game_events_raw().unwrap(),
        build,
    ));
    chunks.sort_by_key(|c| c.ready_loop);
    println!(
        "session {session} : {} blocs, build {build}, vitesse ×{speed}",
        chunks.len()
    );

    let start = Instant::now();
    let mut latencies_ms: Vec<f64> = Vec::with_capacity(chunks.len());
    for c in &chunks {
        let due = Duration::from_secs_f64(c.ready_loop.max(0) as f64 / LOOPS_PER_SEC / speed);
        if let Some(wait) = due.checked_sub(start.elapsed()) {
            std::thread::sleep(wait);
        }
        let stream = match c.kind {
            StreamKind::Tracker => "tracker",
            StreamKind::Game => "game",
        };
        let sent = Instant::now();
        let mut r = agent
            .post(format!("{url}/api/live/chunk"))
            .header("Authorization", &auth)
            .header("X-Session", &session)
            .header("X-Stream", stream)
            .header("X-Offset", c.offset.to_string())
            .header("X-Build", build.to_string())
            .send(&c.bytes[..])
            .unwrap();
        // La réponse part après le broadcast WS : l'aller-retour majore « morceau reçu →
        // live.updated émis » (critère d'acceptation 2 : < 200 ms).
        latencies_ms.push(sent.elapsed().as_secs_f64() * 1000.0);
        let secs = c.ready_loop as f64 / LOOPS_PER_SEC;
        println!(
            "{:02}:{:02} {stream:7} @{:>7} → {} {}",
            (secs / 60.0) as i64,
            (secs % 60.0) as i64,
            c.offset,
            r.status(),
            r.body_mut().read_to_string().unwrap_or_default()
        );
    }

    latencies_ms.sort_by(f64::total_cmp);
    let pct = |p: f64| latencies_ms[((latencies_ms.len() as f64 - 1.0) * p).round() as usize];
    println!(
        "latence POST /api/live/chunk (aller-retour, {} blocs) : p50 {:.1} ms · p95 {:.1} ms · max {:.1} ms",
        latencies_ms.len(),
        pct(0.5),
        pct(0.95),
        pct(1.0)
    );

    // 3. Fin de partie (apparition de replay.details côté jeu).
    let mut r = agent
        .post(format!("{url}/api/live/end"))
        .header("Authorization", &auth)
        .send_json(serde_json::json!({ "session": session }))
        .unwrap();
    println!(
        "end → {} {}",
        r.status(),
        r.body_mut().read_to_string().unwrap_or_default()
    );
}
