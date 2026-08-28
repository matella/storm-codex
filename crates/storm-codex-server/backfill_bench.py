# Jalon 3 T10 — backfill de l'archive complète vers storm-codex-server + critères d'acceptation.
# Uploader headless concurrent (la GUI client-rs étant impraticable à piloter en CI).
# Usage : python backfill_bench.py <archive_dir> [token] [lobby_blob]
#
# [lobby_blob] (optionnel, ou variable d'environnement LOBBY_BLOB) : chemin d'un blob de lobby brut,
# pour mesurer POST /api/lobby (companion live) en plus du backfill. Sans lui, cette passe est
# sautée proprement — le reste du harnais tourne normalement. Pour produire un blob depuis un
# replay déjà archivé :
#   cargo run -q -p storm-replay --example dump_lobby -- "<replay.StormReplay>" /tmp/lobby.bin
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
        body = r.read()
        return r.status, json.loads(body) if body else None


def delete(path, headers=None):
    req = urllib.request.Request(BASE + path, headers=headers or {}, method="DELETE")
    with urllib.request.urlopen(req, timeout=30) as r:
        return r.status, r.read()


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


def bench_lobby(token, blob_path):
    """/api/lobby (companion live) : seul budget perf que la spec attache à cette fonctionnalité
    (p95 < 100 ms), et jusqu'ici le seul endpoint que ce harnais ne mesurait pas — un chiffre
    documenté ailleurs n'était donc appuyé par aucun outil du dépôt. Deux chemins, deux coûts :
    GET sert l'état depuis la mémoire (pas de SQL) ; POST décode le blob ET exécute
    l'enrichissement SQL — c'est celui qui porte le budget.
    """
    print("\n=== /api/lobby (companion live) ===")
    lat_get = []
    for _ in range(30):
        t = time.perf_counter()
        get("/api/lobby")
        lat_get.append((time.perf_counter() - t) * 1000)
    lat_get.sort()
    p95_get = lat_get[int(0.95 * len(lat_get)) - 1]
    print(f"GET  /api/lobby : n={len(lat_get)} médiane={statistics.median(lat_get):.1f}ms "
          f"p95={p95_get:.1f}ms max={lat_get[-1]:.1f}ms (état en mémoire, pas de SQL)")

    if not blob_path:
        print("POST /api/lobby : SAUTÉ (aucun blob fourni — 3e argument ou variable LOBBY_BLOB).")
        print('  Pour en produire un : cargo run -q -p storm-replay --example dump_lobby -- '
              '"<replay.StormReplay>" /tmp/lobby.bin')
        return
    if not os.path.exists(blob_path):
        print(f"POST /api/lobby : SAUTÉ (blob introuvable : {blob_path}).")
        return

    with open(blob_path, "rb") as f:
        blob = f.read()
    headers = {"Authorization": f"Bearer {token}", "Content-Type": "application/octet-stream"}
    lat_post = []
    for _ in range(20):
        # DELETE avant chaque POST : sans ça, à partir du 2e envoi le serveur juge le blob
        # identique au lobby déjà enregistré (mêmes BattleTags) et répond `unchanged` sans
        # ré-enrichir — on ne mesurerait alors que la comparaison en mémoire du 2e appel, pas le
        # budget réel (décodage + enrichissement SQL) que ce test doit prouver.
        delete("/api/lobby")
        t = time.perf_counter()
        code, _ = post("/api/lobby", blob, headers)
        lat_post.append((time.perf_counter() - t) * 1000)
        if code not in (200, 202):
            print(f"  ⚠️ POST /api/lobby a répondu {code}")
    lat_post.sort()
    p95_post = lat_post[int(0.95 * len(lat_post)) - 1]
    print(f"POST /api/lobby : n={len(lat_post)} médiane={statistics.median(lat_post):.1f}ms "
          f"p95={p95_post:.1f}ms max={lat_post[-1]:.1f}ms (décodage + enrichissement SQL)")


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

    lobby_blob = sys.argv[3] if len(sys.argv) > 3 else os.environ.get("LOBBY_BLOB")
    bench_lobby(token, lobby_blob)


if __name__ == "__main__":
    main()
