# Events — WebSocket `/ws` and Jarvis contract

## WebSocket `/ws` (internal broadcast — site, OBS overlays)

Channel `AppState.events` (tokio broadcast, capacity 1024). Types emitted today:

| `type` | Emitted when | Useful fields | Consumers |
|---|---|---|---|
| `match.parsed` | a match projection succeeded | `match_id`, `map`… | SPA (invalidates TanStack queries), OBS widget |
| `draft.updated` | any mutation of the draft simulator | (re-fetch `/api/draft`) | `/draft` console, `/draft/replay` page, `/draft/overlay` overlay |
| `cast.updated` | any mutation of the cast control room | (**no state**: re-fetch `/api/cast`) | cast control room pages |
| `patch.new` | new patch detected by the reference-data job | `internalId`, `name` | in-app notification (WhatsNew) |
| `live.updated` | a live feed chunk that adds events, new session, end, close | `session` (**no state**: the filtered view is read via `GET /api/live`, so that nothing outside the visibility level goes through the WS) | `/live` page |

Client-side contract: events are **re-fetch signals**, not complete data —
a lagging client (RecvError::Lagged) carries on with no functional loss.
Adding an event type = adding it to this table.

## Jarvis (Redis pub/sub) — `jarvis.rs`

Opt-in (empty `REDIS_URL` = no-op; best-effort: a Redis outage never breaks the parse).
Production channel on the box: `storm-codex:match_completed` (`JARVIS_CHANNEL`).

Event `hots.match.completed`, **spine** invariants respected:

```json
{
  "schema_version": 1,
  "type": "hots.match.completed",
  "correlation_id": "<uuid>", "causation_id": "<uuid>",
  "occurred_at": "<rfc3339>", "recorded_at": "<rfc3339>",
  "data": {
    "match_id": 123, "map": "…", "mode": 50101, "length": 536.2, "winner": 1,
    "players": [ { "hero", "name", "team", "win",
                   "kda": {"kills","deaths","takedowns"}, "heroDamage", "healing" } ]
  }
}
```

⚠️ **Jarvis boundary**: the spine's Event model only accepts **one dot** in the type — the
bridge on the Jarvis side (`jarvis/ingest/hots_matches.py`, Jarvis repo) adapts
`hots.match.completed` → `hots.match_completed` at ingestion. Do not "fix" the emitter:
the two-dot format is the spine invariant on the storm-codex side; the adaptation belongs to the
consumer.

Downstream chain (Jarvis repo): bridge → the spine's `events` table → French-language brief worker
(`jarvis/notify/hots_brief.py`, operator perspective via `HOTS_PLAYER_NAME`) → ntfy.

## "New patch" webhook (outgoing, optional)

`PATCH_WEBHOOK_URL`: POST JSON `{content, patchName, internalId}` (Discord-compatible format)
on every detected patch. Best-effort.

## Azure / Twitch extension — DORMANT

`azure.rs` exists but is not wired in (operator decision 2026-06-13: local overlay
only). Do not connect it without a decision to the contrary.
