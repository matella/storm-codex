# Jalon 0 — exporte les typeinfos des protocoles heroprotocol en JSON pour le spike Rust.
# Source : clone du repo GitHub Blizzard/heroprotocol (le package pip est en retard : 91756
# vs 96477 sur GitHub). Usage : python spike/export_protocols.py <chemin du clone>
import importlib.util
import json
import os
import sys
from pathlib import Path

OUT = Path(__file__).parent / "protocols"

CONSTANTS = [
    "tracker_eventid_typeid",
    "svaruint32_typeid",
    "replay_userid_typeid",
    "replay_header_typeid",
    "game_details_typeid",
    "replay_initdata_typeid",
]


def load_module(path):
    spec = importlib.util.spec_from_file_location(path.stem, path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def main():
    clone = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(os.environ["TEMP"]) / "heroprotocol"
    versions = clone / "heroprotocol" / "versions"
    files = sorted(versions.glob("protocol*.py"))
    if not files:
        sys.exit(f"aucun protocole dans {versions}")
    OUT.mkdir(exist_ok=True)
    exported = 0
    for f in files:
        base_build = int(f.stem.removeprefix("protocol"))
        try:
            mod = load_module(f)
        except Exception as e:
            print(f"skip {f.name}: {type(e).__name__}: {e}")
            continue
        data = {"base_build": base_build, "typeinfos": mod.typeinfos}
        for c in CONSTANTS:
            if hasattr(mod, c):
                data[c] = getattr(mod, c)
        if hasattr(mod, "tracker_event_types"):
            data["tracker_event_types"] = {
                str(k): list(v) for k, v in mod.tracker_event_types.items()
            }
        with open(OUT / f"{base_build}.json", "w", encoding="utf-8") as fh:
            json.dump(data, fh)
        exported += 1
    latest = max(int(p.stem) for p in OUT.glob("*.json"))
    (OUT / "latest.txt").write_text(str(latest))
    print(f"{exported} protocoles exportés vers {OUT} (latest = {latest})")


if __name__ == "__main__":
    main()
