//! Jalon 0 — spike : `storm-decode <replay>` (résumé JSON) ou `storm-decode --bench <dir>`
//! (bench aligné sur bench_python.py / bench-dotnet : mono-thread, warm-up exclu,
//! header + details + tracker events, un échec n'arrête pas le bench).

mod protocol;
mod versioned;

use anyhow::{anyhow, Context, Result};
use protocol::ProtocolStore;
use std::path::{Path, PathBuf};
use std::time::Instant;
use versioned::Value;

struct Summary {
    base_build: u32,
    signature: String,
    elapsed_game_loops: i64,
    map: String,
    players: Vec<(String, String, i64)>, // (nom, héros, result 1=victoire 2=défaite)
    tracker_events: usize,
    stats_events: usize,
    score_events: usize,
}

fn decode_file(store: &mut ProtocolStore, path: &Path) -> Result<Summary> {
    let bytes = std::fs::read(path).with_context(|| format!("lecture {}", path.display()))?;
    let (_, mpq) = nom_mpq::parser::parse(&bytes).map_err(|e| anyhow!("MPQ : {e:?}"))?;

    let user_data = mpq
        .user_data
        .as_ref()
        .ok_or_else(|| anyhow!("pas de section user data (header)"))?;
    let header = store.latest()?.decode_header(&user_data.content)?;
    let signature = header
        .field("m_signature")
        .and_then(Value::as_str_lossy)
        .ok_or_else(|| anyhow!("m_signature absent"))?;
    let version = header.field("m_version").ok_or_else(|| anyhow!("m_version absent"))?;
    let base_build = version
        .field("m_baseBuild")
        .and_then(Value::as_int)
        .ok_or_else(|| anyhow!("m_baseBuild absent"))? as u32;
    let elapsed_game_loops = header
        .field("m_elapsedGameLoops")
        .and_then(Value::as_int)
        .unwrap_or(0);

    let proto = store.for_build(base_build)?;

    let (_, details_bytes) = mpq
        .read_mpq_file_sector("replay.details", false, &bytes)
        .map_err(|e| anyhow!("replay.details : {e:?}"))?;
    let details = proto.decode_details(&details_bytes)?;
    let map = details
        .field("m_title")
        .and_then(Value::as_str_lossy)
        .ok_or_else(|| anyhow!("m_title absent"))?;
    let mut players = Vec::new();
    if let Some(Value::Array(list)) = details.field("m_playerList") {
        for p in list {
            players.push((
                p.field("m_name").and_then(Value::as_str_lossy).unwrap_or_default(),
                p.field("m_hero").and_then(Value::as_str_lossy).unwrap_or_default(),
                p.field("m_result").and_then(Value::as_int).unwrap_or(-1),
            ));
        }
    }

    let (_, tracker_bytes) = mpq
        .read_mpq_file_sector("replay.tracker.events", false, &bytes)
        .map_err(|e| anyhow!("replay.tracker.events : {e:?}"))?;
    let events = proto.decode_tracker_events(&tracker_bytes)?;
    let count = |suffix: &str| events.iter().filter(|(n, _)| n.ends_with(suffix)).count();

    Ok(Summary {
        base_build,
        signature,
        elapsed_game_loops,
        map,
        players,
        tracker_events: events.len(),
        stats_events: count("SStatGameEvent"),
        score_events: count("SScoreResultEvent"),
    })
}

fn summary_json(s: &Summary) -> serde_json::Value {
    serde_json::json!({
        "base_build": s.base_build,
        "signature": s.signature.trim_end_matches('\0'),
        "elapsed_game_loops": s.elapsed_game_loops,
        "map": s.map,
        "players": s.players.iter().map(|(name, hero, result)| serde_json::json!({
            "name": name, "hero": hero, "result": result,
        })).collect::<Vec<_>>(),
        "tracker_events": s.tracker_events,
        "stats_events": s.stats_events,
        "score_events": s.score_events,
    })
}

fn bench(store: &mut ProtocolStore, dir: &Path) -> Result<()> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "StormReplay"))
        .collect();
    files.sort();
    anyhow::ensure!(files.len() == 50, "corpus inattendu : {} fichiers", files.len());

    decode_file(store, &files[0]).ok(); // warm-up (caches protocoles), mesure jetée

    let mut rows = vec!["name,ms,base_build,events,ok".to_owned()];
    let mut times = Vec::new();
    let mut fails = 0;
    for f in &files {
        let name = f.file_name().expect("nom de fichier").to_string_lossy().into_owned();
        let t0 = Instant::now();
        let res = decode_file(store, f);
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        match res {
            Ok(s) => {
                times.push(ms);
                rows.push(format!("{name},{ms:.1},{},{},1", s.base_build, s.tracker_events));
            }
            Err(e) => {
                fails += 1;
                eprintln!("FAIL {name}: {e:#}");
                rows.push(format!("{name},{ms:.1},0,0,0"));
            }
        }
    }
    let out = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../bench-results"));
    std::fs::create_dir_all(&out)?;
    std::fs::write(out.join("rust.csv"), rows.join("\n") + "\n")?;

    times.sort_by(|a, b| a.partial_cmp(b).expect("pas de NaN"));
    let median = times[times.len() / 2];
    let p95 = times[(0.95 * times.len() as f64).ceil() as usize - 1];
    println!(
        "rust : n={} échecs={fails} médiane={median:.1} ms p95={p95:.1} ms max={:.1} ms",
        times.len(),
        times.last().expect("au moins une mesure"),
    );
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let default_protocols = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../protocols"));
    let mut store = ProtocolStore::open(default_protocols)?;
    match args.as_slice() {
        [flag, dir] if flag == "--bench" => bench(&mut store, Path::new(dir)),
        [file] => {
            let s = decode_file(&mut store, Path::new(file))?;
            println!("{}", serde_json::to_string_pretty(&summary_json(&s))?);
            Ok(())
        }
        _ => Err(anyhow!("usage : storm-decode (<replay> | --bench <dir>)")),
    }
}
