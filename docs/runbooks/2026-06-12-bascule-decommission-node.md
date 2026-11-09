# Runbook — storm-codex switchover & decommissioning of the local Node server (milestone 5)

> **Box/operator** step (in the evening, box on). The code is ready; this switchover is
> operational. To run when the storm-codex server is running on the box and the front-end is
> validated. No loss of function expected (Twitch Azure EBS kept, fed by push).

## Prerequisites
- `storm-codex-server` deployed on the box (rsync + `docker compose build`), the box's Postgres + Redis
  configured (`DATABASE_URL`, `REDIS_URL`), `WEB_DIR` = built front-end, archive backfilled.
- `client-rs` re-pointed (`SERVER_URL` → storm-codex) deployed on the gaming PC.
- Push variables: `JARVIS_CHANNEL` (Jarvis Redis channel), `AZURE_PUSH_URL`/`AZURE_PUSH_TOKEN`
  (same values as the existing patch-digest push).

## Switchover
1. **Check functional equivalence** before stopping the Node:
   - a test game → page up to date < 5 s (WS), OBS widget up to date, `hots.match.completed` event
     received on the Jarvis Redis (`redis-cli subscribe <JARVIS_CHANNEL>`), Twitch extension up to date
     (Azure push ok).
   - OBS overlay re-pointed to `https://<box>/widget` (browser source).
2. **Inspect the overlay's Mongo one last time** (the spec notes: empty to date,
   0 replays):
   ```
   docker exec <mongo> mongosh --eval 'db.matches.countDocuments()'
   ```
   If it contains non-derivable definitions (test teams/collections), recreate them
   by hand in the storm-codex UI (milestone 4) before deletion.
3. **Stop the local Node server** (`Hots-Overlay/server.js`):
   ```
   docker compose -f <overlay>/docker-compose.yml stop   # or pm2 stop / systemctl stop
   ```
4. **Delete the Mongo** once the inspection is done and the overlay stopped.

## What REMAINS (by design, V1)
- **Twitch Azure EBS**: kept, fed by storm-codex's outgoing `box→Azure` push
  (`azure.rs`). Twitch JWT validation stays in the Azure EBS (absorption = V2).
- **HotsPatchNotes** (:5001): hero/talent reference data, consumed by storm-codex at startup
  (milestone 4 — `dim_*` replication). Unchanged.

## Rollback
The local Node can be restarted as long as the Mongo is not deleted. Keep an observation
window (a few evenings) before the final deletion of the Mongo.
