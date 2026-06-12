//! Dump JSON de storm-stats, comparé à tools/parity-harness/dump.js (hots-parser).
//! Usage : storm-stats-dump <replay> <sortie.json> [--filename <chaîne>]
//!         storm-stats-dump --bench <dir>   (decode + stats, mono-thread, warm-up exclu)

use std::path::{Path, PathBuf};
use std::time::Instant;

#[cfg(feature = "fast-alloc")]
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let [flag, dir] = args.as_slice() {
        if flag == "--bench" {
            bench(Path::new(dir));
            return;
        }
    }
    let (file, out, filename) = match args.as_slice() {
        [file, out] => (file.clone(), out.clone(), file.clone()),
        [file, out, flag, name] if flag == "--filename" => {
            (file.clone(), out.clone(), name.clone())
        }
        _ => {
            eprintln!("usage : storm-stats-dump <replay> <sortie.json> [--filename <chaîne>]");
            eprintln!("        storm-stats-dump --bench <dir>");
            std::process::exit(2);
        }
    };
    let output = storm_stats::process_replay(Path::new(&file), &filename);
    match serde_json::to_string(&output.to_json()) {
        Ok(s) => {
            if let Err(e) = std::fs::write(&out, s) {
                eprintln!("écriture {out} : {e}");
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("sérialisation : {e}");
            std::process::exit(1);
        }
    }
}

/// decode + stats par replay (mono-thread, warm-up exclu). Sépare les replays au parse
/// complet (status OK) des rejets (cartes hors MapType, etc.) — seuls les premiers reflètent
/// le coût réel du pipeline complet.
fn bench(dir: &Path) {
    let mut files: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "StormReplay"))
            .collect(),
        Err(e) => {
            eprintln!("lecture {} : {e}", dir.display());
            std::process::exit(1);
        }
    };
    files.sort();
    if let Some(first) = files.first() {
        let _ = storm_stats::process_replay(first, "warmup"); // mesure jetée
    }
    let mut full = Vec::new();
    let mut rejected = 0usize;
    for f in &files {
        let name = f.to_string_lossy().into_owned();
        let t0 = Instant::now();
        let out = storm_stats::process_replay(f, &name);
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        if out.status == 1 {
            full.push(ms);
        } else {
            rejected += 1;
        }
    }
    if full.is_empty() {
        eprintln!("aucun replay au parse complet");
        std::process::exit(1);
    }
    full.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let pct = |p: f64| full[((p * full.len() as f64).ceil() as usize).saturating_sub(1)];
    println!(
        "storm-stats decode+stats : n={} (rejets {rejected}) médiane={:.1} ms p95={:.1} ms max={:.1} ms",
        full.len(),
        full[full.len() / 2],
        pct(0.95),
        full.last().copied().unwrap_or(0.0),
    );
}
