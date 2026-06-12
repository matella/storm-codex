//! Dump JSON de storm-stats, comparé à tools/parity-harness/dump.js (hots-parser).
//! Usage : storm-stats-dump <replay> <sortie.json> [--filename <chaîne>]

use std::path::Path;

#[cfg(feature = "fast-alloc")]
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (file, out, filename) = match args.as_slice() {
        [file, out] => (file.clone(), out.clone(), file.clone()),
        [file, out, flag, name] if flag == "--filename" => {
            (file.clone(), out.clone(), name.clone())
        }
        _ => {
            eprintln!("usage : storm-stats-dump <replay> <sortie.json> [--filename <chaîne>]");
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
