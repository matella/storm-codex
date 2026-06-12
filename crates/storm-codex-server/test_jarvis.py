# Test jalon 5 T1 : abonné Jarvis fiable. Subscribe, confirme, déclenche un upload frais,
# capture l'event hots.match.completed et valide les invariants spine.
import glob, hashlib, json, os, subprocess, sys, time
import redis

R = redis.Redis(host="127.0.0.1", port=6380)
ps = R.pubsub()
ps.subscribe("jarvis:events")
# avale la confirmation d'abonnement
assert ps.get_message(timeout=5)["type"] == "subscribe"

# force un parse frais : supprime l'upload d'un fichier puis ré-upload
f = glob.glob(os.path.expanduser("~/Documents/Heroes of the Storm/Accounts/**/*.StormReplay"), recursive=True)
# prend un fichier au hasard parsable (carte classique)
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
        print("OK event Jarvis : type =", e["type"])
        print("invariants spine présents :", inv)
        print("carte :", e["data"]["map"], "| joueurs :", len(e["data"]["players"]))
        print("exemple joueur :", e["data"]["players"][0]["hero"], e["data"]["players"][0]["kda"])
        sys.exit(0 if inv and len(e["data"]["players"]) == 10 else 1)
print("ÉCHEC : pas d'event en 10 s")
sys.exit(1)
