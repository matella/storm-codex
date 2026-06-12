# Jalon 3 T10 — backfill de l'archive complète vers storm-codex-server + critères d'acceptation.
# Uploader headless concurrent (la GUI client-rs étant impraticable à piloter en CI).
# Usage : python backfill_bench.py <archive_dir> [token]
import concurrent.futures as cf
import glob
import json
import os
import statistics
import sys
import time
import urllib.request

BASE = "http://127.0.0.1:8088"


def post(path, data, headers):
    req = urllib.request.Request(BASE + path, data=data, headers=headers, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            return r.status, r.read()
    except urllib.error.HTTPError as e:
        return e.code, e.read()


def get(path, headers=None):
    req = urllib.request.Request(BASE + path, headers=headers or {})
    with urllib.request.urlopen(req, timeout=30) as r:
        return r.status, json.loads(r.read())


def admin_token():
    s, b = post("/api/admin/tokens", json.dumps({"name": "backfill"}).encode(),
                {"Authorization": "Bearer dev-admin-token", "Content-Type": "application/json"})
    return json.loads(b)["token"]


def upload_one(path, token):
    with open(path, "rb") as f:
        data = f.read()
    code, _ = post("/api/upload", data,
                   {"Authorization": f"Bearer {token}", "Content-Type": "application/octet-stream"})
    return code


def main():
    archive = sys.argv[1]
    files = glob.glob(os.path.join(archive, "**", "*.StormReplay"), recursive=True)
    print(f"{len(files)} replays à backfiller")
    token = sys.argv[2] if len(sys.argv) > 2 else admin_token()

    t0 = time.perf_counter()
    sent = 0
    with cf.ThreadPoolExecutor(max_workers=16) as ex:
        for code in ex.map(lambda p: upload_one(p, token), files):
            sent += 1
            if sent % 250 == 0:
                print(f"  envoyés {sent}/{len(files)}")
    print(f"tous envoyés en {time.perf_counter() - t0:.0f}s ; attente fin des parses…")

    # attendre qu'il n'y ait plus de 'pending'
    while True:
        _, h = get("/api/admin/uploads", {"Authorization": "Bearer dev-admin-token"})
        pending = h["by_status"].get("pending", 0)
        if pending == 0:
            break
        time.sleep(2)
    elapsed = time.perf_counter() - t0
    _, h = get("/api/admin/uploads", {"Authorization": "Bearer dev-admin-token"})

    print(f"\n=== BACKFILL TERMINÉ en {elapsed:.0f}s ({elapsed / 60:.1f} min) ===")
    print("par statut :", h["by_status"])
    print("par classe d'erreur :", h["by_error_class"])
    total = sum(h["by_status"].values())
    parsed = h["by_status"].get("parsed", 0)
    # « tentés » = tout sauf pending ; « parsés » inclut les rejets connus classés
    failed = h["by_status"].get("parse_failed", 0)
    print(f"total tentés : {total} | parsés : {parsed} ({100 * parsed / total:.1f}%) | "
          f"échecs classés : {failed} ({100 * failed / total:.1f}%)")

    # p95 API (lecture)
    _, matches = get("/api/matches?limit=50")
    ids = [m["id"] for m in matches][:30]
    lat = []
    for _ in range(3):
        for path in ["/api/matches?limit=50", "/api/heroes"] + [f"/api/matches/{i}" for i in ids]:
            t = time.perf_counter()
            get(path)
            lat.append((time.perf_counter() - t) * 1000)
    lat.sort()
    p95 = lat[int(0.95 * len(lat)) - 1]
    print(f"API lecture : n={len(lat)} médiane={statistics.median(lat):.1f}ms "
          f"p95={p95:.1f}ms max={lat[-1]:.1f}ms")


if __name__ == "__main__":
    main()
