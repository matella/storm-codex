# Jalon 0 — baseline heroprotocol (Python).
# NB : heroprotocol 2.55.15.96477 (pip) est cassé sur Python >= 3.12 (versions/__init__.py
# importe `imp`, supprimé) — on charge les modules protocolXXXXX.py directement via importlib.
import csv
import importlib.util
import math
import time
from pathlib import Path

import mpyq

PKG = Path(importlib.util.find_spec("heroprotocol").origin).parent
VERSIONS = PKG / "versions"
CORPUS = Path(__file__).parent.parent / "corpus" / "spike50"
OUT = Path(__file__).parent / "bench-results"

_cache = {}


def load_protocol(name):
    if name not in _cache:
        spec = importlib.util.spec_from_file_location(name, VERSIONS / f"{name}.py")
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        _cache[name] = mod
    return _cache[name]


LATEST = load_protocol(sorted(p.stem for p in VERSIONS.glob("protocol*.py"))[-1])


def decode(path):
    """Chaîne canonique de hero_cli.py : header (latest) -> protocole du build -> streams."""
    archive = mpyq.MPQArchive(str(path))
    header = LATEST.decode_replay_header(archive.header["user_data_header"]["content"])
    base_build = header["m_version"]["m_baseBuild"]
    try:
        protocol = load_protocol("protocol%05d" % base_build)
    except FileNotFoundError:
        protocol = LATEST  # fallback « dernier protocole connu » (comportement spec)
    protocol.decode_replay_details(archive.read_file("replay.details"))
    events = sum(
        1 for _ in protocol.decode_replay_tracker_events(archive.read_file("replay.tracker.events"))
    )
    return base_build, events


def main():
    files = sorted(CORPUS.glob("*.StormReplay"))
    assert len(files) == 50, f"corpus inattendu : {len(files)} fichiers"
    decode(files[0])  # warm-up (imports/JIT caches), mesure jetée
    rows, fails = [], 0
    for f in files:
        t0 = time.perf_counter()
        try:
            base_build, events = decode(f)
            ok = 1
        except Exception as e:  # un échec n'arrête pas le bench
            base_build, events, ok = 0, 0, 0
            fails += 1
            print(f"FAIL {f.name}: {type(e).__name__}: {e}")
        ms = (time.perf_counter() - t0) * 1000
        rows.append({"name": f.name, "ms": f"{ms:.1f}", "base_build": base_build,
                     "events": events, "ok": ok})
    OUT.mkdir(exist_ok=True)
    with open(OUT / "python.csv", "w", newline="", encoding="utf-8") as fh:
        w = csv.DictWriter(fh, fieldnames=["name", "ms", "base_build", "events", "ok"])
        w.writeheader()
        w.writerows(rows)
    ms = sorted(float(r["ms"]) for r in rows if r["ok"])
    p95 = ms[math.ceil(0.95 * len(ms)) - 1]
    builds = sorted({r["base_build"] for r in rows if r["ok"]})
    print(f"python : n={len(ms)} échecs={fails} médiane={ms[len(ms) // 2]:.0f} ms "
          f"p95={p95:.0f} ms max={ms[-1]:.0f} ms")
    print(f"builds du corpus : {builds}")


if __name__ == "__main__":
    main()
