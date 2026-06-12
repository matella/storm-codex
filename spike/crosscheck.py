# Jalon 0 — cross-check : sortie storm-decode (Rust) vs heroprotocol (Python) sur les
# 3 replays les plus récents du corpus. Champ par champ : build, carte, joueurs
# (nom/héros/résultat), nombre de tracker events, elapsed_game_loops.
import json
import subprocess
import sys
from pathlib import Path

from bench_python import LATEST, load_protocol  # réutilise le loader (pip cassé sur 3.12+)
import mpyq

ROOT = Path(__file__).parent.parent
CORPUS = ROOT / "corpus" / "spike50"
EXE = ROOT / "spike" / "storm-decode" / "target" / "release" / "storm-decode.exe"


def python_summary(path):
    archive = mpyq.MPQArchive(str(path))
    header = LATEST.decode_replay_header(archive.header["user_data_header"]["content"])
    base_build = header["m_version"]["m_baseBuild"]
    try:
        protocol = load_protocol("protocol%05d" % base_build)
    except FileNotFoundError:
        protocol = LATEST
    details = protocol.decode_replay_details(archive.read_file("replay.details"))
    events = list(protocol.decode_replay_tracker_events(archive.read_file("replay.tracker.events")))
    return {
        "base_build": base_build,
        "elapsed_game_loops": header["m_elapsedGameLoops"],
        "map": details["m_title"].decode("utf-8"),
        "players": [
            {
                "name": p["m_name"].decode("utf-8"),
                "hero": p["m_hero"].decode("utf-8"),
                "result": p["m_result"],
            }
            for p in details["m_playerList"]
        ],
        "tracker_events": len(events),
    }


def rust_summary(path):
    out = subprocess.run([str(EXE), str(path)], capture_output=True, check=True)
    s = json.loads(out.stdout)
    s.pop("signature", None)
    s.pop("stats_events", None)
    s.pop("score_events", None)
    return s


def main():
    files = sorted(CORPUS.glob("*.StormReplay"))[-3:]
    assert len(files) == 3
    ok = 0
    for f in files:
        py, rs = python_summary(f), rust_summary(f)
        if py == rs:
            ok += 1
            print(f"OK   {f.name}")
        else:
            print(f"DIFF {f.name}")
            for k in sorted(set(py) | set(rs)):
                if py.get(k) != rs.get(k):
                    print(f"  {k}:\n    python: {py.get(k)}\n    rust  : {rs.get(k)}")
    print(f"{ok}/3 OK")
    sys.exit(0 if ok == 3 else 1)


if __name__ == "__main__":
    main()
