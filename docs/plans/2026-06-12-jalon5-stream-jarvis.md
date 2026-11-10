# Milestone 5 — stream + Jarvis + switchover

> **For Claude:** REQUIRED SUB-SKILL: superpowers:executing-plans.

**Goal:** post-game OBS widget, `hots.match.completed` event → Redis (Jarvis spine
invariants), post-game push box→Azure (Twitch extension), decommissioning of the local Node server.

**Accept (spec):** game played → OBS widget + Jarvis brief + Twitch extension up to date; local Node
server stopped without loss of function.

**Buildable here (without the box)**: OBS widget, Redis emitter (testable against a local Docker Redis),
Azure push code (config, untested against the real EBS). **Box/operator**: real Jarvis Redis,
Jarvis consumer, Azure EBS, stopping the local Node.

---

### Task 1: Jarvis emitter (Redis) + spine invariants
- `redis` in the compose (dev) + `redis` crate. `REDIS_URL` config (optional; absent = no emission).
- `jarvis.rs`: on each `match.parsed`, publishes `hots.match.completed` to Redis with the
  invariants: `schema_version`, `correlation_id`/`causation_id`, `occurred_at`/`recorded_at`,
  past-tense `entity.verb` type. Payload: result, hero, KDA, map, duration, awards, gaps.
- Test: publishes against local Redis, a subscriber receives the well-formed event. Commit.

### Task 2: OBS widget (standalone page)
- `/widget`: page (served by the binary) summarising the last game / session, transparent background,
  live update via WS `match.parsed`. Last game looked up via `/api/matches?limit=1`.
- Check rendering. Commit.

### Task 3: post-game push box→Azure (code + config)
- `azure.rs`: authenticated outgoing POST (same mechanism as the HotsPatchNotes patch-digest) of the
  post-game summary to the Azure EBS (`AZURE_PUSH_URL`/token, optional). Untested against the
  real EBS — flagged. Commit.

### Task 4: Node decommissioning + STATUS + push
- Document stopping the local Node server (Mongo inspection, stop, deletion) — box step,
  scripted but to be run by the operator in the evening. STATUS, commit, push.
