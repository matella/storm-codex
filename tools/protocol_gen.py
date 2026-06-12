# protocol-gen — génère les tables de protocole de storm-replay depuis Blizzard/heroprotocol.
# À relancer à chaque patch HotS :
#   python tools/protocol_gen.py            (clone auto dans %TEMP%/heroprotocol si absent)
#   python tools/protocol_gen.py --clone-dir <chemin d'un clone existant>
#
# IMPORTANT : source = clone GitHub, jamais le package PyPI (en retard de ~5000 builds et
# cassé sur Python >= 3.12 — voir docs/research/2026-06-12-jalon0-bench.md).
#
# Sortie (committée) : crates/storm-replay/protocols/
#   tables/<hash12>.json  — contenus distincts (390 builds -> ~32 tables)
#   index.json            — build -> hash, + latest
#   embed.rs              — include_str! de toutes les tables (généré, ne pas éditer)
import argparse
import hashlib
import importlib.util
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).parent.parent
OUT = ROOT / "crates" / "storm-replay" / "protocols"

CONSTANTS = [
    "tracker_eventid_typeid",
    "game_eventid_typeid",
    "message_eventid_typeid",
    "svaruint32_typeid",
    "replay_userid_typeid",
    "replay_header_typeid",
    "game_details_typeid",
    "replay_initdata_typeid",
]
EVENT_TABLES = ["tracker_event_types", "game_event_types", "message_event_types"]


def load_module(path: Path):
    spec = importlib.util.spec_from_file_location(path.stem, path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def export_one(path: Path) -> dict:
    mod = load_module(path)
    data = {"typeinfos": mod.typeinfos}
    for c in CONSTANTS:
        if hasattr(mod, c):
            data[c] = getattr(mod, c)
    for t in EVENT_TABLES:
        if hasattr(mod, t):
            data[t] = {str(k): list(v) for k, v in getattr(mod, t).items()}
    return data


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--clone-dir", type=Path,
                    default=Path(os.environ.get("TEMP", "/tmp")) / "heroprotocol")
    args = ap.parse_args()

    if not (args.clone_dir / "heroprotocol" / "versions").is_dir():
        print(f"clone de Blizzard/heroprotocol vers {args.clone_dir}…")
        subprocess.run(
            ["git", "clone", "--depth", "1",
             "https://github.com/Blizzard/heroprotocol.git", str(args.clone_dir)],
            check=True,
        )
    versions = args.clone_dir / "heroprotocol" / "versions"

    tables_dir = OUT / "tables"
    if tables_dir.exists():
        shutil.rmtree(tables_dir)
    tables_dir.mkdir(parents=True)

    builds: dict[int, str] = {}
    contents: dict[str, str] = {}  # hash12 -> json canonique
    skipped = []
    for f in sorted(versions.glob("protocol*.py")):
        build = int(f.stem.removeprefix("protocol"))
        try:
            data = export_one(f)
        except Exception as e:
            skipped.append((f.name, f"{type(e).__name__}: {e}"))
            continue
        canon = json.dumps(data, sort_keys=True, separators=(",", ":"))
        h = hashlib.sha256(canon.encode()).hexdigest()[:12]
        contents.setdefault(h, canon)
        builds[build] = h

    if skipped:
        for name, err in skipped:
            print(f"skip {name}: {err}", file=sys.stderr)
    if not builds:
        sys.exit("aucun protocole exporté")

    for h, canon in contents.items():
        (tables_dir / f"{h}.json").write_text(canon, encoding="utf-8", newline="\n")

    latest = max(builds)
    index = {"latest": latest, "builds": {str(b): h for b, h in sorted(builds.items())}}
    (OUT / "index.json").write_text(
        json.dumps(index, indent=1) + "\n", encoding="utf-8", newline="\n"
    )

    lines = [
        "// Généré par tools/protocol_gen.py — NE PAS ÉDITER (relancer le script).",
        f"pub const LATEST_BUILD: u32 = {latest};",
        "pub static TABLES: &[(&str, &str)] = &[",
    ]
    for h in sorted(contents):
        lines.append(f'    ("{h}", include_str!("tables/{h}.json")),')
    lines.append("];")
    lines.append("pub static BUILDS: &[(u32, &str)] = &[")
    for b, h in sorted(builds.items()):
        lines.append(f'    ({b}, "{h}"),')
    lines.append("];")
    (OUT / "embed.rs").write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")

    size = sum(p.stat().st_size for p in tables_dir.glob("*.json"))
    print(f"{len(builds)} builds -> {len(contents)} tables ({size / 1e6:.2f} MB), "
          f"latest = {latest}")


if __name__ == "__main__":
    main()
