# Milestone 5 T1 test: reliable Jarvis subscriber. Subscribe, confirm, trigger a fresh upload,
# capture the hots.match.completed event and validate the spine invariants.
import glob, hashlib, json, os, subprocess, sys, time
import redis

R = redis.Redis(host="127.0.0.1", port=6380)
ps = R.pubsub()
ps.subscribe("jarvis:events")
# swallow the subscription confirmation
assert ps.get_message(timeout=5)["type"] == "subscribe"

# force a fresh parse: delete the upload of a file then re-upload
f = glob.glob(os.path.expanduser("~/Documents/Heroes of the Storm/Accounts/**/*.StormReplay"), recursive=True)
# take a random parsable file (classic map)
import random; random.seed(7)
target = next(p for p in f if "Sky Temple" in p or "Cursed Hollow" in p)
ch = hashlib.sha256(open(target, "rb").read()).hexdigest()
subprocess.run(["docker", "exec", "storm-codex-pg", "psql", "-U", "storm", "-d", "storm_codex",
                "-c", f"DELETE FROM uploads WHERE fingerprint='{ch}';"], capture_output=True)
subprocess.run(["curl", "-s", "-X", "POST", "http://127.0.0.1:8088/api/upload",
                "-H", "Authorization: Bearer devtoken", "--data-binary", f"@{target}"], capture_output=True)

deadline = time.time() + 10
while time.time() < deadline:
    msg = ps.get_message(timeout=2)
    if not msg or msg["type"] != "message":
        continue
    e = json.loads(msg["data"])
    if e.get("type") == "hots.match.completed":
        inv = all(k in e for k in ["schema_version", "correlation_id", "causation_id", "occurred_at", "recorded_at"])
        print("OK Jarvis event: type =", e["type"])
        print("spine invariants present:", inv)
        print("map:", e["data"]["map"], "| players:", len(e["data"]["players"]))
        print("example player:", e["data"]["players"][0]["hero"], e["data"]["players"][0]["kda"])
        sys.exit(0 if inv and len(e["data"]["players"]) == 10 else 1)
print("FAILED: no event within 10 s")
sys.exit(1)
