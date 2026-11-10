//! Extracts the `replay.server.battlelobby` stream of a .StormReplay to a raw file.
//! It is the same blob as the one written live by the game in
//! `%TEMP%\Heroes of the Storm\TempWriteReplayP1\replay.server.battlelobby`.
//!
//! Usage: cargo run -p storm-replay --example dump_lobby -- <replay> <output.bin>
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let usage = || anyhow::anyhow!("usage: dump_lobby <replay.StormReplay> <output.bin>");
    let input: PathBuf = args.next().ok_or_else(usage)?.into();
    let output: PathBuf = args.next().ok_or_else(usage)?.into();

    let replay = storm_replay::Replay::open(&input)?;
    let blob = replay.battlelobby_raw()?;
    std::fs::write(&output, &blob)?;
    println!("{} bytes → {}", blob.len(), output.display());
    Ok(())
}
