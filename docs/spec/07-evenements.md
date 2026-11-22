# Events — WebSocket `/ws`

## WebSocket `/ws` (internal broadcast — site, OBS overlays)

Channel `AppState.events` (tokio broadcast, capacity 1024). Types emitted today:

| `type` | Emitted when | Useful fields | Consumers |
|---|---|---|---|
| `match.parsed` | a match projection succeeded | `match_id`, `map`… | SPA (invalidates TanStack queries), OBS widget |
| `draft.updated` | any mutation of the draft simulator | (re-fetch `/api/draft`) | `/draft` console, `/draft/replay` page, `/draft/overlay` overlay |
| `cast.updated` | any mutation of the cast control room | (**no state**: re-fetch `/api/cast`) | cast control room pages |
| `patch.new` | new patch detected by the box reference-data job | `internalId`, `name` | in-app notification (WhatsNew) |
| `live.updated` | a live feed chunk that adds events, new session, end, close | `session` (**no state**: the filtered view is read via `GET /api/live`, so that nothing outside the visibility level goes through the WS) | `/live` page |

Client-side contract: events are **re-fetch signals**, not complete data —
a lagging client (RecvError::Lagged) carries on with no functional loss.
Adding an event type = adding it to this table.

Standalone reference activation uses the opaque string `revision` in the local
`GET /api/standalone/reference` status as the refetch signal, even when the patch IDs did not
change. It does not emit `patch.new`.

## "New patch" webhook (outgoing, optional)

`PATCH_WEBHOOK_URL`: POST JSON `{content, patchName, internalId}` (Discord-compatible format)
on every detected patch. Best-effort.

## Azure / Twitch extension — DORMANT

`azure.rs` exists but is not wired in (operator decision 2026-06-13: local overlay
only). Do not connect it without a decision to the contrary.
