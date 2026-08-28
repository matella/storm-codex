//! Extrait le stream `replay.server.battlelobby` d'un .StormReplay vers un fichier brut.
//! C'est le même blob que celui écrit en live par le jeu dans
//! `%TEMP%\Heroes of the Storm\TempWriteReplayP1\replay.server.battlelobby`.
//!
//! Usage : cargo run -p storm-replay --example dump_lobby -- <replay> <sortie.bin>
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let usage = || anyhow::anyhow!("usage: dump_lobby <replay.StormReplay> <sortie.bin>");
    let input: PathBuf = args.next().ok_or_else(usage)?.into();
    let output: PathBuf = args.next().ok_or_else(usage)?.into();

    let replay = storm_replay::Replay::open(&input)?;
    let blob = replay.battlelobby_raw()?;
    std::fs::write(&output, &blob)?;
    println!("{} octets → {}", blob.len(), output.display());
    Ok(())
}
