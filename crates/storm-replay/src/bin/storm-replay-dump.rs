//! Dump JSON d'un stream de replay, normalisé « comme heroprotocol » (blobs en latin-1,
//! bitarrays bitpacked en hex, reals en `[f]`) — consommé par tools/crosscheck_streams.py.
//! Modes :
//!   storm-replay-dump <replay> --stream header|details|initdata|attributes|tracker|game|message
//!   storm-replay-dump --bench <dir>   (7 streams, mono-thread, warm-up exclu)

use std::path::{Path, PathBuf};
use std::time::Instant;
use storm_replay::{Attributes, Replay, Value};

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

/// hex(int) Python : "0x0", "0x1f4"…
fn hex_big(bytes: &[u8]) -> String {
    match bytes.iter().position(|&b| b != 0) {
        None => "0x0".into(),
        Some(i) => {
            let mut s = format!("0x{:x}", bytes[i]);
            for b in &bytes[i + 1..] {
                s.push_str(&format!("{b:02x}"));
            }
            s
        }
    }
}

fn to_json(v: &Value) -> serde_json::Value {
    use serde_json::json;
    match v {
        Value::Null => serde_json::Value::Null,
        Value::Int(i) => json!(i),
        Value::Bool(b) => json!(b),
        Value::Real(f) => json!([f]),
        Value::Blob(b) => json!(latin1(b)),
        Value::Fourcc(b) => json!(latin1(b)),
        Value::Array(items) => serde_json::Value::Array(items.iter().map(to_json).collect()),
        Value::BitArrayBytes { bits, data } => json!([bits, latin1(data)]),
        Value::BitArrayInt { bits, value } => json!([bits, hex_big(value)]),
        Value::Struct(fields) => serde_json::Value::Object(
            fields.iter().map(|(n, v)| (n.clone(), to_json(v))).collect(),
        ),
    }
}

fn attributes_json(a: &Attributes) -> serde_json::Value {
    use serde_json::json;
    let scopes: serde_json::Map<String, serde_json::Value> = a
        .scopes
        .iter()
        .map(|(scope, attrs)| {
            let inner: serde_json::Map<String, serde_json::Value> = attrs
                .iter()
                .map(|(attrid, values)| {
                    (
                        attrid.to_string(),
                        serde_json::Value::Array(
                            values
                                .iter()
                                .map(|v| {
                                    json!({
                                        "namespace": v.namespace,
                                        "attrid": v.attrid,
                                        "value": latin1(&v.value),
                                    })
                                })
                                .collect(),
                        ),
                    )
                })
                .collect();
            (scope.to_string(), serde_json::Value::Object(inner))
        })
        .collect();
    json!({"source": a.source, "mapNamespace": a.map_namespace, "scopes": scopes})
}

fn run() -> storm_replay::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [flag, dir] if flag == "--bench" => bench(Path::new(dir)),
        [file, flag, stream] if flag == "--stream" => dump(Path::new(file), stream),
        _ => {
            eprintln!(
                "usage : storm-replay-dump <replay> --stream <nom> | storm-replay-dump --bench <dir>"
            );
            std::process::exit(2);
        }
    }
}

fn dump(path: &Path, stream: &str) -> storm_replay::Result<()> {
    let replay = Replay::open(path)?;
    let print = |v: serde_json::Value| println!("{v}");
    let print_events = |events: Vec<Value>| {
        for e in &events {
            println!("{}", to_json(e));
        }
    };
    match stream {
        "header" => print(to_json(&replay.header_raw)),
        "details" => print(to_json(&replay.details_raw()?)),
        "initdata" => print(to_json(&replay.initdata_raw()?)),
        "attributes" => print(attributes_json(&replay.attributes()?)),
        "tracker" => print_events(replay.tracker_events()?),
        "game" => print_events(replay.game_events()?),
        "message" => print_events(replay.message_events()?),
        other => {
            eprintln!("stream inconnu : {other}");
            std::process::exit(2);
        }
    }
    Ok(())
}

fn decode_all_streams(path: &Path) -> storm_replay::Result<usize> {
    let replay = Replay::open(path)?;
    let mut n = replay.details_raw().map(|_| 1)?;
    replay.initdata_raw()?;
    replay.attributes()?;
    n += replay.tracker_events()?.len();
    n += replay.game_events()?.len();
    n += replay.message_events()?.len();
    Ok(n)
}

fn bench(dir: &Path) -> storm_replay::Result<()> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "StormReplay"))
        .collect();
    files.sort();

    if let Some(first) = files.first() {
        decode_all_streams(first).ok(); // warm-up (cache des tables), mesure jetée
    }
    let mut rows = vec!["name,ms,ok".to_owned()];
    let mut times = Vec::new();
    let mut fails = 0;
    for f in &files {
        let name = f.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let t0 = Instant::now();
        let res = decode_all_streams(f);
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        match res {
            Ok(_) => {
                times.push(ms);
                rows.push(format!("{name},{ms:.1},1"));
            }
            Err(e) => {
                fails += 1;
                eprintln!("FAIL {name}: {e}");
                rows.push(format!("{name},{ms:.1},0"));
            }
        }
    }
    let out = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../spike/bench-results"));
    std::fs::create_dir_all(&out)?;
    std::fs::write(out.join("rust-jalon1.csv"), rows.join("\n") + "\n")?;

    times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if times.is_empty() {
        eprintln!("aucune mesure");
        std::process::exit(1);
    }
    let median = times[times.len() / 2];
    let p95 = times[(0.95 * times.len() as f64).ceil() as usize - 1];
    println!(
        "rust 7 streams : n={} échecs={fails} médiane={median:.1} ms p95={p95:.1} ms max={:.1} ms",
        times.len(),
        times.last().copied().unwrap_or_default(),
    );
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("erreur : {e}");
        std::process::exit(1);
    }
}
