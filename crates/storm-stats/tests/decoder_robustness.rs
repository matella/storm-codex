//! Decoder robustness (security review H2): mutation-fuzz-style regression over every committed
//! replay. The replay header (MPQ user data), `replay.details` (versioned) and `replay.initData`
//! (bitpacked) are mutated — targeted byte flips, the exact security review C1 payload (a struct
//! field count of 2^35), huge array/struct counts spliced after container tags, a deep
//! unknown-field nesting, and seeded random flips — and every case must come back from
//! `storm_replay::Replay::from_bytes` + `storm_stats::process_replay` as a value (`Ok`/`Err`,
//! status), never a panic, an allocation abort, a stack overflow or a hang.
//!
//! Each case runs in a child process (this test binary re-invoked on `child_entry`), the way the
//! review reproduced C1: an abort kills the child only, the parent records the culprit case and
//! carries on with the next one in a new child.
//!
//! The mutated `details`/`initData` are stored **uncompressed** in the archive (the block table
//! entry is rewritten and re-encrypted), so the mutations reach the decoders instead of only
//! breaking the zlib stream; `rewritten_archive_is_faithful` checks that the rewrite itself
//! changes nothing.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const CHILD_ENV: &str = "STORM_FUZZ_CHILD_RANGE";
const MARK: &str = "@@fuzz";
/// Per case, generous for a debug build (a full parse is well under 5 s).
const CASE_TIMEOUT: Duration = Duration::from_secs(60);
const RANDOM_CASES_PER_TARGET: u64 = 6;
const WORKERS: usize = 4;

const MPQ_FILE_EXISTS: u32 = 0x8000_0000;
const MPQ_FILE_SINGLE_UNIT: u32 = 0x0100_0000;
/// Security review C1: struct tag + vint field count 2^35.
const C1_PAYLOAD: [u8; 7] = [0x05, 0x80, 0x80, 0x80, 0x80, 0x80, 0x02];
const HUGE_VINT: [u8; 6] = [0x80, 0x80, 0x80, 0x80, 0x80, 0x02];

fn replays() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut out: Vec<PathBuf> = std::fs::read_dir(root.join("../storm-replay/tests/data"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "StormReplay"))
        .collect();
    out.sort();
    out.push(root.join("tests/data/silver-city-aram.StormReplay"));
    out
}

#[derive(Clone, Copy, Debug)]
enum Target {
    Header,
    Details,
    InitData,
}

#[derive(Clone, Debug)]
enum Mutation {
    /// `bytes[at] ^= 0xFF`
    Flip(usize),
    /// overwrite from `at` (clamped to the end)
    Splice(usize, Vec<u8>),
    /// unknown struct field + `levels` nested "optional present" skips (versioned only)
    DeepSkip(usize),
    /// `n` seeded random byte replacements
    Random(u64),
}

#[derive(Clone, Debug)]
struct Case {
    replay: usize,
    target: Target,
    mutation: Mutation,
}

fn xorshift(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

/// Header = MPQ user data content: `MPQ\x1b` + 3 LE u32, the last one its size; content at 16.
fn header_range(archive: &[u8]) -> std::ops::Range<usize> {
    let size = u32::from_le_bytes(archive[12..16].try_into().unwrap()) as usize;
    16..16 + size
}

fn stream_name(target: Target) -> &'static str {
    match target {
        Target::Header => unreachable!(),
        Target::Details => "replay.details",
        Target::InitData => "replay.initData",
    }
}

fn original_target_bytes(archive: &[u8], target: Target) -> Vec<u8> {
    match target {
        Target::Header => archive[header_range(archive)].to_vec(),
        _ => {
            let (_, mpq) = nom_mpq::parser::parse(archive).unwrap();
            mpq.read_mpq_file_sector(stream_name(target), false, archive)
                .unwrap()
                .1
        }
    }
}

/// Deterministic list of cases (same order in parent and children).
fn cases(archives: &[Vec<u8>]) -> Vec<Case> {
    let mut out = Vec::new();
    for (replay, archive) in archives.iter().enumerate() {
        for target in [Target::Header, Target::Details, Target::InitData] {
            let data = original_target_bytes(archive, target);
            let len = data.len();
            let mut push = |mutation| {
                out.push(Case {
                    replay,
                    target,
                    mutation,
                })
            };
            for at in [0, 1, 2, 3, 5, 8, 13, 21, 34, len / 3, len / 2, len - 1] {
                push(Mutation::Flip(at.min(len - 1)));
            }
            if !matches!(target, Target::InitData) {
                // versioned: C1 itself, then huge counts after the first container tags
                push(Mutation::Splice(0, C1_PAYLOAD.to_vec()));
                let containers = data
                    .iter()
                    .enumerate()
                    .filter(|(_, &b)| b == 0x00 || b == 0x05 || b == 0x02)
                    .map(|(i, _)| i)
                    .take(4);
                for at in containers.collect::<Vec<_>>() {
                    push(Mutation::Splice(at + 1, HUGE_VINT.to_vec()));
                }
                push(Mutation::DeepSkip(if matches!(target, Target::Header) {
                    len.saturating_sub(8) / 2
                } else {
                    200_000
                }));
            } else {
                // bitpacked: saturate the leading length fields
                push(Mutation::Splice(0, vec![0xFF; 16]));
                push(Mutation::Splice(len / 2, vec![0xFF; 16]));
            }
            for seed in 1..=RANDOM_CASES_PER_TARGET {
                push(Mutation::Random(
                    seed * 0x9E37_79B9 + replay as u64 * 31 + target as u64,
                ));
            }
        }
    }
    out
}

fn mutate(data: &mut Vec<u8>, mutation: &Mutation) {
    let len = data.len();
    match mutation {
        Mutation::Flip(at) => data[*at] ^= 0xFF,
        Mutation::Splice(at, bytes) => {
            for (i, b) in bytes.iter().enumerate() {
                if let Some(slot) = data.get_mut(at + i) {
                    *slot = *b;
                }
            }
        }
        Mutation::DeepSkip(levels) => {
            // struct, 1 field, unknown tag 63, then the nesting, then a byte value
            let mut bytes = vec![0x05, 0x02, 0x7E];
            for _ in 0..*levels {
                bytes.extend_from_slice(&[0x04, 0x01]);
            }
            bytes.extend_from_slice(&[0x06, 0x00]);
            if bytes.len() <= len {
                data[..bytes.len()].copy_from_slice(&bytes); // header: fixed size
            } else {
                *data = bytes;
            }
        }
        Mutation::Random(seed) => {
            let mut state = *seed | 1;
            let n = 1 + xorshift(&mut state) % 4;
            for _ in 0..n {
                let at = (xorshift(&mut state) % len as u64) as usize;
                data[at] = xorshift(&mut state) as u8;
            }
        }
    }
}

/// MPQ block/hash table encryption (inverse of `nom_mpq::MPQ::mpq_data_decrypt`).
fn encrypt(table: &HashMap<u32, u32>, plain: &[u8], key: u32) -> Vec<u8> {
    let (mut seed1, mut seed2) = (key, 0xEEEE_EEEEu32);
    let mut out = Vec::with_capacity(plain.len());
    for chunk in plain.as_chunks::<4>().0 {
        let value = u32::from_le_bytes(*chunk);
        seed2 = seed2.wrapping_add(table[&(0x400 + (seed1 & 0xFF))]);
        out.extend_from_slice(&(value ^ seed1.wrapping_add(seed2)).to_le_bytes());
        seed1 = ((!seed1) << 0x15).wrapping_add(0x1111_1111) | (seed1 >> 0x0B);
        seed2 = value
            .wrapping_add(seed2)
            .wrapping_add(seed2 << 5)
            .wrapping_add(3);
    }
    out
}

/// The archive with the embedded file `name` replaced by `content`, stored uncompressed at the
/// end (its block table entry rewritten and re-encrypted).
fn with_stream(archive: &[u8], name: &str, content: &[u8]) -> Vec<u8> {
    let (_, mpq) = nom_mpq::parser::parse(archive).unwrap();
    let idx = mpq.get_hash_table_entry(name).unwrap().block_table_index as usize;
    let header = &mpq.archive_header;
    let start = header.offset + header.block_table_offset as usize;
    let mut out = archive.to_vec();
    let rel = u32::try_from(out.len() - header.offset).unwrap();
    out.extend_from_slice(content);
    let len = u32::try_from(content.len()).unwrap();
    let mut plain = Vec::new();
    for (i, b) in mpq.block_table_entries.iter().enumerate() {
        let entry = if i == idx {
            [rel, len, len, MPQ_FILE_EXISTS | MPQ_FILE_SINGLE_UNIT]
        } else {
            [b.offset, b.archived_size, b.size, b.flags]
        };
        for v in entry {
            plain.extend_from_slice(&v.to_le_bytes());
        }
    }
    let key = nom_mpq::MPQ::mpq_string_hash(
        &mpq.encryption_table,
        "(block table)",
        nom_mpq::parser::MPQHashType::Table,
    )
    .unwrap();
    let encrypted = encrypt(&mpq.encryption_table, &plain, key);
    out[start..start + encrypted.len()].copy_from_slice(&encrypted);
    out
}

fn build_case(archive: &[u8], case: &Case) -> Vec<u8> {
    let mut data = original_target_bytes(archive, case.target);
    mutate(&mut data, &case.mutation);
    match case.target {
        Target::Header => {
            let mut out = archive.to_vec();
            out[header_range(archive)].copy_from_slice(&data);
            out
        }
        target => with_stream(archive, stream_name(target), &data),
    }
}

fn load_archives() -> Vec<Vec<u8>> {
    replays()
        .iter()
        .map(|p| std::fs::read(p).unwrap())
        .collect()
}

/// Child side: runs the cases `start..end` of the env range, one marker line before and after
/// each (stdout, flushed) so the parent knows which case killed the process.
#[test]
fn child_entry() {
    let Ok(range) = std::env::var(CHILD_ENV) else {
        return; // normal test run: nothing to do
    };
    let (start, end) = range.split_once("..").unwrap();
    let (start, end): (usize, usize) = (start.parse().unwrap(), end.parse().unwrap());
    let archives = load_archives();
    let all = cases(&archives);
    let dir = std::env::temp_dir();
    let mut stdout = std::io::stdout();
    for (i, case) in all.iter().enumerate().take(end).skip(start) {
        writeln!(stdout, "{MARK} begin {i}").unwrap();
        stdout.flush().unwrap();
        let bytes = build_case(&archives[case.replay], case);
        let decoded = match storm_replay::Replay::from_bytes(bytes.clone()) {
            Ok(r) => format!(
                "details={} initdata={}",
                r.details_raw().is_ok(),
                r.initdata_raw().is_ok()
            ),
            Err(e) => format!("from_bytes error ({e})"),
        };
        let path = dir.join(format!("storm-fuzz-{}-{i}.StormReplay", std::process::id()));
        std::fs::write(&path, &bytes).unwrap();
        let status = storm_stats::process_replay(&path, "fuzz.StormReplay").status;
        let _ = std::fs::remove_file(&path);
        let decoded: String = decoded.chars().take(120).collect();
        writeln!(stdout, "{MARK} end {i} status={status} {decoded}").unwrap();
        stdout.flush().unwrap();
    }
}

enum Line {
    Begin(usize),
    End(usize, String),
}

/// Runs the cases `range` in children; a child that dies (or hangs) is blamed on its in-flight
/// case and a new child resumes after it. Returns (case, outcome) for every case.
fn run_range(range: std::ops::Range<usize>) -> Vec<(usize, Result<String, String>)> {
    let exe = std::env::current_exe().unwrap();
    let mut results = Vec::new();
    let mut next = range.start;
    while next < range.end {
        let mut child = Command::new(&exe)
            .args(["child_entry", "--exact", "--nocapture", "--test-threads=1"])
            .env(CHILD_ENV, format!("{next}..{}", range.end))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let err_reader = std::thread::spawn(move || {
            let mut s = String::new();
            let _ = std::io::Read::read_to_string(&mut BufReader::new(stderr), &mut s);
            s
        });
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                // libtest prints "test child_entry ... " without a newline before the first marker
                let Some(rest) = line.find(MARK).map(|at| &line[at + MARK.len()..]) else {
                    continue;
                };
                let mut parts = rest.trim().splitn(3, ' ');
                let (kind, i) = (
                    parts.next().unwrap(),
                    parts.next().unwrap().parse().unwrap(),
                );
                let msg = parts.next().unwrap_or_default().to_owned();
                let _ = tx.send(if kind == "begin" {
                    Line::Begin(i)
                } else {
                    Line::End(i, msg)
                });
            }
        });
        let mut in_flight: Option<(usize, Instant)> = None;
        let mut hung = false;
        loop {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(Line::Begin(i)) => in_flight = Some((i, Instant::now())),
                Ok(Line::End(i, msg)) => {
                    results.push((i, Ok(msg)));
                    in_flight = None;
                    next = i + 1;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if in_flight.is_some_and(|(_, t)| t.elapsed() > CASE_TIMEOUT) {
                        hung = true;
                        let _ = child.kill();
                        break;
                    }
                }
            }
        }
        let status = child.wait().unwrap();
        let stderr = err_reader.join().unwrap_or_default();
        match in_flight {
            Some((i, _)) => {
                let tail: String = stderr
                    .lines()
                    .rev()
                    .take(6)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>()
                    .join(" | ");
                let why = if hung {
                    "hang (killed)"
                } else {
                    "process died"
                };
                results.push((i, Err(format!("{why}: {status}: {tail}"))));
                next = i + 1;
            }
            None if !status.success() => {
                panic!("child failed outside a case ({status}): {stderr}");
            }
            None => {}
        }
    }
    results
}

#[test]
fn mutated_replays_never_crash_the_process() {
    if std::env::var_os(CHILD_ENV).is_some() {
        return;
    }
    let archives = load_archives();
    let total = cases(&archives).len();
    assert!(total >= 150, "{total} cases");
    let chunk = total.div_ceil(WORKERS);
    let handles: Vec<_> = (0..WORKERS)
        .map(|w| {
            let range = (w * chunk).min(total)..((w + 1) * chunk).min(total);
            std::thread::spawn(move || run_range(range))
        })
        .collect();
    let mut results: Vec<_> = handles
        .into_iter()
        .flat_map(|h| h.join().unwrap())
        .collect();
    results.sort_by_key(|(i, _)| *i);
    let all = cases(&archives);
    let failures: Vec<String> = results
        .iter()
        .filter_map(|(i, r)| r.as_ref().err().map(|e| format!("#{i} {:?}: {e}", all[*i])))
        .collect();
    let rejected = results
        .iter()
        .filter(|(_, r)| r.as_ref().is_ok_and(|m| !m.starts_with("status=1 ")))
        .count();
    eprintln!(
        "{} cases run, {} crashed, {rejected} rejected cleanly",
        results.len(),
        failures.len()
    );
    assert_eq!(results.len(), total, "every case ran exactly once");
    assert!(
        failures.is_empty(),
        "{} case(s) crashed the process:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The harness itself: re-storing the untouched `details`/`initData` uncompressed gives the same
/// stats, so the mutations really reach the decoders.
#[test]
fn rewritten_archive_is_faithful() {
    if std::env::var_os(CHILD_ENV).is_some() {
        return;
    }
    let path = &replays()[0];
    let archive = std::fs::read(path).unwrap();
    let mut rewritten = archive.clone();
    for target in [Target::Details, Target::InitData] {
        let data = original_target_bytes(&archive, target);
        rewritten = with_stream(&rewritten, stream_name(target), &data);
    }
    assert_ne!(rewritten, archive);
    let tmp =
        std::env::temp_dir().join(format!("storm-fuzz-rt-{}.StormReplay", std::process::id()));
    std::fs::write(&tmp, &rewritten).unwrap();
    let ours = storm_stats::process_replay(&tmp, "rt.StormReplay");
    let _ = std::fs::remove_file(&tmp);
    let theirs = storm_stats::process_replay(path, "rt.StormReplay");
    assert_eq!(ours.status, 1);
    assert_eq!(ours.to_json(), theirs.to_json());
}
