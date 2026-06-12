# Jalon 2 — diff de parité storm-stats (Rust) vs hots-parser (Node, étalon).
# Usage : python tools/parity-harness/diff.py --corpus corpus/stats [--only match.map,players.*.name]
#         [--limit N] [--replay <nom>]
# Les dumps de référence Node sont cachés dans <corpus>/.ref/ (hots-parser ~1-3 s/replay).
# Tolérances (= écarts assumés, documentés) : tools/parity-harness/tolerances.json.
import argparse
import json
import math
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).parent.parent.parent
NODE_DUMP = Path(__file__).parent / "dump.js"
RUST_DUMP = ROOT / "target" / "release" / "storm-stats-dump.exe"
TOLERANCES = Path(__file__).parent / "tolerances.json"


def load_tolerances():
    if TOLERANCES.exists():
        return [t["path"].split(".") for t in json.loads(TOLERANCES.read_text(encoding="utf-8"))]
    return []


def tolerated(path_parts, tolerances):
    for tol in tolerances:
        if len(tol) == len(path_parts) and all(
            t == "*" or t == p for t, p in zip(tol, path_parts)
        ):
            return True
    return False


def ref_dump(replay: Path, ref_dir: Path) -> dict | None:
    out = ref_dir / (replay.name + ".json")
    if not out.exists():
        r = subprocess.run(
            ["node", str(NODE_DUMP), str(replay), str(out)],
            capture_output=True,
            text=True,
        )
        if r.returncode != 0:
            # statut non-OK : on le mémorise pour comparer le statut côté Rust
            err = r.stderr.strip().splitlines()
            status = None
            for line in err:
                if line.startswith("statut "):
                    status = int(line.split()[1])
            out.write_text(json.dumps({"status": status}), encoding="utf-8")
    try:
        return json.loads(out.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return None


def rust_dump(replay: Path, tmp: Path) -> dict:
    out = tmp / (replay.name + ".rust.json")
    subprocess.run(
        [str(RUST_DUMP), str(replay), str(out), "--filename", str(replay)],
        check=True,
        capture_output=True,
    )
    return json.loads(out.read_text(encoding="utf-8"))


def is_num(v):
    return isinstance(v, (int, float)) and not isinstance(v, bool)


def matches_only(path_parts, only):
    """Un chemin est comparé s'il est sur la trajectoire d'un préfixe --only (segments, * ok)."""
    for o in only:
        oseg = o.split(".")
        n = min(len(oseg), len(path_parts))
        if all(s == "*" or s == p for s, p in zip(oseg[:n], path_parts[:n])):
            return True
    return False


def diff_values(ref, act, path, out, tolerances, only):
    """Collecte les écarts (réf = hots-parser, act = storm-stats)."""
    if len(out) >= 200:
        return
    if tolerated(path, tolerances):
        return
    if only and path and not matches_only(path, only):
        return
    # null ≡ absent géré par les appels (clé manquante -> None)
    if ref is None and act is None:
        return
    if is_num(ref) and is_num(act):
        if not math.isclose(ref, act, rel_tol=1e-6, abs_tol=1e-6):
            out.append((".".join(path), ref, act))
        return
    if type(ref) is not type(act):
        out.append((".".join(path), ref, act))
        return
    if isinstance(ref, dict):
        for k in set(ref) | set(act):
            diff_values(ref.get(k), act.get(k), path + [k], out, tolerances, only)
        return
    if isinstance(ref, list):
        if len(ref) != len(act):
            out.append((".".join(path) + ".length", len(ref), len(act)))
            return
        for i, (r, a) in enumerate(zip(ref, act)):
            diff_values(r, a, path + [str(i)], out, tolerances, only)
        return
    if ref != act:
        out.append((".".join(path), ref, act))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--corpus", type=Path, default=ROOT / "corpus" / "stats")
    ap.add_argument("--only", type=str, default=None, help="préfixes de chemins, séparés par ,")
    ap.add_argument("--limit", type=int, default=None)
    ap.add_argument("--replay", type=str, default=None, help="filtre sur le nom de fichier")
    ap.add_argument("--max-diffs", type=int, default=8, help="écarts affichés par replay")
    args = ap.parse_args()

    only = args.only.split(",") if args.only else None
    tolerances = load_tolerances()
    ref_dir = args.corpus / ".ref"
    ref_dir.mkdir(exist_ok=True)
    tmp = args.corpus / ".rust"
    tmp.mkdir(exist_ok=True)

    files = sorted(args.corpus.glob("*.StormReplay"))
    if args.replay:
        files = [f for f in files if args.replay.lower() in f.name.lower()]
    if args.limit:
        files = files[: args.limit]

    ok = fail = skipped = 0
    for f in files:
        ref = ref_dump(f, ref_dir)
        if ref is None:
            print(f"SKIP {f.name} (référence illisible)")
            skipped += 1
            continue
        act = rust_dump(f, tmp)
        if ref.get("status") != 1:
            # hots-parser rejette : storm-stats doit rejeter avec le même statut
            if act.get("status") == ref.get("status"):
                ok += 1
                print(f"OK   {f.name} (rejet identique, statut {ref.get('status')})")
            else:
                fail += 1
                print(f"DIFF {f.name} : statut {ref.get('status')} vs {act.get('status')}")
            continue
        diffs = []
        diff_values(ref, act, [], diffs, tolerances, only)
        if not diffs:
            ok += 1
            print(f"OK   {f.name}")
        else:
            fail += 1
            print(f"DIFF {f.name} ({len(diffs)} écarts)")
            for path, r, a in diffs[: args.max_diffs]:
                rs, as_ = json.dumps(r, ensure_ascii=False), json.dumps(a, ensure_ascii=False)
                print(f"  {path}:\n    node: {rs[:200]}\n    rust: {as_[:200]}")
    print(f"\n{ok} OK · {fail} DIFF · {skipped} SKIP / {len(files)}")
    sys.exit(1 if fail else 0)


if __name__ == "__main__":
    main()
