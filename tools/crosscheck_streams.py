# Milestone 1 — stream-by-stream parity: storm-replay (Rust) vs heroprotocol (Python).
# Deep-compare of the 7 streams on the 3 most recent replays of the spike50 corpus + the 2024
# replay of the mini-corpus. Usage: python tools/crosscheck_streams.py
import importlib.util
import json
import os
import subprocess
import sys
from pathlib import Path

import mpyq

ROOT = Path(__file__).parent.parent
EXE = ROOT / "target" / "release" / "storm-replay-dump.exe"
VERSIONS = Path(os.environ.get("TEMP", "/tmp")) / "heroprotocol" / "heroprotocol" / "versions"

_cache = {}


def load_protocol(name):
    if name not in _cache:
        spec = importlib.util.spec_from_file_location(name, VERSIONS / f"{name}.py")
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        _cache[name] = mod
    return _cache[name]


LATEST = load_protocol(sorted(p.stem for p in VERSIONS.glob("protocol*.py"))[-1])


def normalize(v):
    """Python heroprotocol -> canonical form comparable to the Rust dump."""
    if isinstance(v, bytes):
        return v.decode("latin-1")
    if isinstance(v, tuple):
        if len(v) == 2 and isinstance(v[1], bytes):  # bitarray versioned
            return [v[0], v[1].decode("latin-1")]
        if len(v) == 2 and isinstance(v[1], int):  # bitarray bitpacked
            return [v[0], hex(v[1])]
        return [normalize(x) for x in v]  # reals (f,) etc.
    if isinstance(v, list):
        return [normalize(x) for x in v]
    if isinstance(v, dict):
        return {str(k): normalize(x) for k, x in v.items() if k != "_bits"}
    return v


def py_streams(path):
    archive = mpyq.MPQArchive(str(path))
    header = LATEST.decode_replay_header(archive.header["user_data_header"]["content"])
    base_build = header["m_version"]["m_baseBuild"]
    try:
        proto = load_protocol("protocol%05d" % base_build)
    except FileNotFoundError:
        proto = LATEST
    read = archive.read_file
    return {
        "header": [header],
        "details": [proto.decode_replay_details(read("replay.details"))],
        "initdata": [proto.decode_replay_initdata(read("replay.initData"))],
        "attributes": [proto.decode_replay_attributes_events(read("replay.attributes.events"))],
        "tracker": list(proto.decode_replay_tracker_events(read("replay.tracker.events"))),
        "game": list(proto.decode_replay_game_events(read("replay.game.events"))),
        "message": list(proto.decode_replay_message_events(read("replay.message.events"))),
    }


def rust_stream(path, stream):
    out = subprocess.run(
        [str(EXE), str(path), "--stream", stream],
        capture_output=True,
        check=True,
        text=True,
        encoding="utf-8",
    )
    # not splitlines(): it also splits on \x85/\x1c... present in the latin-1 blobs
    return [json.loads(line) for line in out.stdout.split("\n") if line.strip()]


def first_diff(a, b, path="$"):
    """Returns a description of the first difference, or None."""
    if type(a) is not type(b) and not (isinstance(a, (int, float)) and isinstance(b, (int, float))):
        return f"{path}: types {type(a).__name__} vs {type(b).__name__} ({a!r} vs {b!r})"
    if isinstance(a, dict):
        for k in sorted(set(a) | set(b)):
            if k not in a:
                return f"{path}.{k}: missing on python side"
            if k not in b:
                return f"{path}.{k}: missing on rust side"
            d = first_diff(a[k], b[k], f"{path}.{k}")
            if d:
                return d
        return None
    if isinstance(a, list):
        if len(a) != len(b):
            return f"{path}: longueurs {len(a)} vs {len(b)}"
        for i, (x, y) in enumerate(zip(a, b)):
            d = first_diff(x, y, f"{path}[{i}]")
            if d:
                return d
        return None
    if a != b:
        return f"{path}: {a!r} != {b!r}"
    return None


def main():
    spike = sorted((ROOT / "corpus" / "spike50").glob("*.StormReplay"))[-3:]
    mini2024 = sorted((ROOT / "crates/storm-replay/tests/data").glob("2024*.StormReplay"))
    files = spike + mini2024
    assert len(files) == 4, f"{len(files)} files"
    failures = 0
    for f in files:
        py = py_streams(f)
        for stream in ["header", "details", "initdata", "attributes", "tracker", "game", "message"]:
            expected = [normalize(e) for e in py[stream]]
            actual = rust_stream(f, stream)
            diff = first_diff(expected, actual)
            if diff is None:
                print(f"OK   {f.name} [{stream}] ({len(expected)} elements)")
            else:
                failures += 1
                print(f"DIFF {f.name} [{stream}]: {diff}")
    print("PARITE OK (7 streams x 4 replays)" if failures == 0 else f"{failures} ECARTS")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
