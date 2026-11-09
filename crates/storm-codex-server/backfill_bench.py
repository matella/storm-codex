# Milestone 3 T10 — backfill of the whole archive into storm-codex-server + acceptance criteria.
# Concurrent headless uploader (the client-rs GUI being impractical to drive in CI).
# Usage: python backfill_bench.py <archive_dir> [token] [lobby_blob]
#
# [lobby_blob] (optional, or the LOBBY_BLOB environment variable): path of a raw lobby blob,
# to measure POST /api/lobby (companion live) on top of the backfill. Without it, this pass is
# cleanly skipped — the rest of the harness runs normally. To produce a blob from an
# already archived replay:
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
    """/api/lobby (companion live): the only perf budget the spec attaches to this feature
    (p95 < 100 ms), and until now the only endpoint this harness did not measure — a figure
    documented elsewhere was therefore backed by no tool of the repository. Two paths, two costs:
    GET serves the state from memory (no SQL); POST decodes the blob AND runs
    the SQL enrichment — it is the one that carries the budget.
    """
    print("\n=== /api/lobby (companion live) ===")

    if not blob_path:
        print("POST /api/lobby: SKIPPED (no blob given — 3rd argument or LOBBY_BLOB variable).")
        print('  To produce one: cargo run -q -p storm-replay --example dump_lobby -- '
              '"<replay.StormReplay>" /tmp/lobby.bin')
        print("GET  /api/lobby: SKIPPED (depends on an enriched state set by the POST above).")
        return
    if not os.path.exists(blob_path):
        print(f"POST /api/lobby: SKIPPED (blob not found: {blob_path}).")
        print("GET  /api/lobby: SKIPPED (depends on an enriched state set by the POST above).")
        return

    with open(blob_path, "rb") as f:
        blob = f.read()
    headers = {"Authorization": f"Bearer {token}", "Content-Type": "application/octet-stream"}
    lat_post = []
    for _ in range(20):
        # DELETE before each POST: without it, from the 2nd send the server considers the blob
        # identical to the lobby already stored (same BattleTags) and answers `unchanged` without
        # re-enriching — we would then only measure the in-memory comparison of the 2nd call, not the
        # real budget (decoding + SQL enrichment) this test must prove.
        delete("/api/lobby")
        t = time.perf_counter()
        code, _ = post("/api/lobby", blob, headers)
        lat_post.append((time.perf_counter() - t) * 1000)
        if code not in (200, 202):
            print(f"  ⚠️ POST /api/lobby answered {code}")
    lat_post.sort()
    p95_post = lat_post[int(0.95 * len(lat_post)) - 1]
    print(f"POST /api/lobby: n={len(lat_post)} median={statistics.median(lat_post):.1f}ms "
          f"p95={p95_post:.1f}ms max={lat_post[-1]:.1f}ms (decoding + SQL enrichment)")

    # Measured AFTER the POST above (never before): the last iteration of the loop
    # ends on a POST, with no DELETE after it — the in-memory state at the time of this GET is therefore
    # the complete enriched state (ten players, histories, build), not a `204` on empty memory. A
    # GET measured before the first POST would prove nothing about the real serialisation cost.
    lat_get = []
    for _ in range(30):
        t = time.perf_counter()
        get("/api/lobby")
        lat_get.append((time.perf_counter() - t) * 1000)
    lat_get.sort()
    p95_get = lat_get[int(0.95 * len(lat_get)) - 1]
    print(f"GET  /api/lobby: n={len(lat_get)} median={statistics.median(lat_get):.1f}ms "
          f"p95={p95_get:.1f}ms max={lat_get[-1]:.1f}ms (enriched state in memory, no SQL)")


def main():
    archive = sys.argv[1]
    files = glob.glob(os.path.join(archive, "**", "*.StormReplay"), recursive=True)
    print(f"{len(files)} replays to backfill")
    token = sys.argv[2] if len(sys.argv) > 2 else admin_token()

    t0 = time.perf_counter()
    sent = 0
    with cf.ThreadPoolExecutor(max_workers=16) as ex:
        for code in ex.map(lambda p: upload_one(p, token), files):
            sent += 1
            if sent % 250 == 0:
                print(f"  sent {sent}/{len(files)}")
    print(f"all sent in {time.perf_counter() - t0:.0f}s; waiting for the parses to finish…")

    # wait until there is no 'pending' left
    while True:
        _, h = get("/api/admin/uploads", {"Authorization": "Bearer dev-admin-token"})
        pending = h["by_status"].get("pending", 0)
        if pending == 0:
            break
        time.sleep(2)
    elapsed = time.perf_counter() - t0
    _, h = get("/api/admin/uploads", {"Authorization": "Bearer dev-admin-token"})

    print(f"\n=== BACKFILL DONE in {elapsed:.0f}s ({elapsed / 60:.1f} min) ===")
    print("by status:", h["by_status"])
    print("by error class:", h["by_error_class"])
    total = sum(h["by_status"].values())
    parsed = h["by_status"].get("parsed", 0)
    # "attempted" = everything but pending; "parsed" includes the known classified rejections
    failed = h["by_status"].get("parse_failed", 0)
    print(f"total attempted: {total} | parsed: {parsed} ({100 * parsed / total:.1f}%) | "
          f"classified failures: {failed} ({100 * failed / total:.1f}%)")

    # API p95 (read)
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
    print(f"API read: n={len(lat)} median={statistics.median(lat):.1f}ms "
          f"p95={p95:.1f}ms max={lat[-1]:.1f}ms")

    lobby_blob = sys.argv[3] if len(sys.argv) > 3 else os.environ.get("LOBBY_BLOB")
    bench_lobby(token, lobby_blob)


if __name__ == "__main__":
    main()
