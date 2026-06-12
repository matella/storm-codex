//! Sweep de validation : décode les 7 streams de chaque `.StormReplay` d'un répertoire
//! (récursif), classe les échecs par type d'erreur. Critère jalon 1 : 100 % décodés.
//! Usage : storm-replay-verify <dir> [--csv <fichier>]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;
use storm_replay::{Error, Replay};

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(&path, out)?;
        } else if path.extension().is_some_and(|x| x == "StormReplay") {
            out.push(path);
        }
    }
    Ok(())
}

fn decode_all(path: &Path) -> storm_replay::Result<(u32, bool)> {
    let replay = Replay::open(path)?;
    replay.details_raw()?;
    replay.initdata_raw()?;
    replay.attributes()?;
    replay.tracker_events()?;
    replay.game_events()?;
    replay.message_events()?;
    Ok((replay.header.base_build, replay.protocol_fallback().is_some()))
}

fn error_class(e: &Error) -> &'static str {
    match e {
        Error::Io(_) => "io",
        Error::Mpq(_) => "mpq",
        Error::MissingStream(..) => "missing_stream",
        Error::Truncated(_) => "truncated",
        Error::Corrupted(_) => "corrupted",
        Error::Protocol(_) => "protocol",
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (dir, csv) = match args.as_slice() {
        [dir] => (PathBuf::from(dir), None),
        [dir, flag, csv] if flag == "--csv" => (PathBuf::from(dir), Some(PathBuf::from(csv))),
        _ => {
            eprintln!("usage : storm-replay-verify <dir> [--csv <fichier>]");
            std::process::exit(2);
        }
    };

    let mut files = Vec::new();
    if let Err(e) = collect(&dir, &mut files) {
        eprintln!("parcours de {} : {e}", dir.display());
        std::process::exit(1);
    }
    files.sort();
    println!("{} replays sous {}", files.len(), dir.display());

    let t0 = Instant::now();
    let mut rows = vec!["name,base_build,fallback,ok,error_class,error".to_owned()];
    let mut by_class: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut by_build: BTreeMap<u32, usize> = BTreeMap::new();
    let mut fallbacks = 0usize;
    let mut ok = 0usize;
    for (i, f) in files.iter().enumerate() {
        let name = f.file_name().unwrap_or_default().to_string_lossy().into_owned();
        match decode_all(f) {
            Ok((build, fallback)) => {
                ok += 1;
                *by_build.entry(build).or_default() += 1;
                if fallback {
                    fallbacks += 1;
                }
                rows.push(format!("{name},{build},{},1,,", u8::from(fallback)));
            }
            Err(e) => {
                let class = error_class(&e);
                *by_class.entry(class).or_default() += 1;
                let msg = e.to_string().replace(',', ";").replace('\n', " ");
                println!("FAIL [{class}] {name}: {msg}");
                rows.push(format!("{name},0,0,0,{class},{msg}"));
            }
        }
        if (i + 1) % 250 == 0 {
            println!("… {}/{} ({} ok)", i + 1, files.len(), ok);
        }
    }
    let secs = t0.elapsed().as_secs_f64();

    if let Some(csv_path) = csv {
        if let Err(e) = std::fs::write(&csv_path, rows.join("\n") + "\n") {
            eprintln!("écriture CSV {} : {e}", csv_path.display());
        }
    }

    println!(
        "\n{ok}/{} décodés ({:.1} %) en {secs:.0} s — {fallbacks} via fallback protocole",
        files.len(),
        100.0 * ok as f64 / files.len().max(1) as f64,
    );
    println!(
        "builds distincts : {} ({} → {})",
        by_build.len(),
        by_build.keys().next().copied().unwrap_or(0),
        by_build.keys().last().copied().unwrap_or(0),
    );
    if !by_class.is_empty() {
        println!("échecs par classe : {by_class:?}");
        std::process::exit(1);
    }
}
