# Match chat — redesign

> storm-codex spec. Scope chosen by the operator on 2026-09-08 (all four parts).
> **Awaiting approval** before `writing-plans` and implementation.

## Goal

The chat feed exists (`ChatLog`, `web/src/pages/MatchDetail.tsx:269`) but remains a passive
list: you read who said what, without ever knowing **where the game stood**. Yet the data to
answer that is already decoded and already served — it is simply not used. The
redesign ties the chat to game time, makes it searchable, and extends search to the whole archive.

## What exists (as of 2026-09-08)

| Element | Where | What it does |
|---|---|---|
| Message extraction | `crates/storm-stats/src/process.rs:1905` (`process_messages_and_bm`) | `type`, `player`, `team`, `recipient`, `loop`, `time`, then `text` / `point{x,y}` / `announcement` |
| Transport | `GET /api/matches/{id}` → `match.messages` | nothing chat-specific, everything comes from the match object |
| Display | `ChatLog` | clock, avatar, coloured name, `all`/`allies`/`obs` badge, text; pings and callouts toggles |
| Ping count per player | `BMTable` (`MatchDetail.tsx:239`) | "Pings" column |

**What the data carries and nothing displays**: the pings' `point{x,y}`
(`process.rs:1953`), even though a 2D viewer with the real minimaps exists in the next
tab.

**Known limitation, kept**: **observer messages are discarded** by the parser (filter
`player_lobby_id`, `process.rs:1936`), in line with hots-parser. No effect on matchmade
modes; visible in custom games. We do not lift it here — that would be a parity divergence
to investigate in `03-storm-stats.md`, outside the subject of this spec.

## Decisions made

1. **No change to the shape of the projection or to `PARSER_VERSION`** for parts A to C:
   everything is already in `match.messages`. Part D adds a **read table**, not a field.
2. The chat stays **in the score tab**, the viewer in its own: we link them, we do not
   merge them.
3. The filtering helpers are **pure functions in `web/src/api.ts`**, tested with vitest
   (convention of `10-developpement.md`: pure helper → vitest test, no contract doc to change).
4. Part C must **not weigh down the loading of the match page**: the context data
   comes from `/api/matches/{id}/replay2d`, fetched **on demand** when the operator enables the
   context, and shared with the viewer tab through the TanStack cache (`["replay2d", id]`,
   `staleTime: Infinity`).

## Part A — Chat ↔ 2D viewer

### A1. Clicking a message positions the viewer

Clicking a message's timestamp switches to the `replay2d` tab, positions playback at
`msg.time` and pauses. A click on the score tab goes back, the feed having kept its position.

Implementation: `Replay2D` currently owns its `t` as local state (`Replay2D.tsx:229`) and is
mounted in only one place (`MatchDetail.tsx:357`). Two **optional** props are added to it
(`t?`, `onSeek?`) — when not provided, the component behaves exactly as today; when provided,
`MatchDetail` holds the state. No rework of `usePlayback`, which already does not own `t`.

### A2. Pings placed on the minimap

Pings are displayed at their real position during a window of a few seconds around
`t`, like deaths (`deathsNear`, `web/src/replay2d.ts:71`).

**Point to check before committing to it**: `m_point`'s frame of reference is not proven identical to
that of hero positions. The viewer normalises by `x / MapSizeX` with coordinates in
tiles ×4096 (`crates/storm-replay-viewer/src/extract.rs:16-21`), and `process.rs:1950` already documents
a **signed** interpretation divergence with hots-parser on this exact field. The first
task of the part is therefore a **check**: overlay the pings of a known replay and confirm
that they land on the map, in the right areas (a "danger" ping near a camp, not out of
frame). If the frame of reference does not match, A2 is **dropped** and A1 delivered alone — no approximate
position displayed as truth.

## Part B — Search and filters

Purely front-end, no API change.

- **Search field** on the text (ignores case and accents).
- **Chips per player** (the match's 10, with avatar) — cumulative.
- **Filter by recipient**: `all` / `allies` / `obs` (the value is already displayed, not yet
  filterable).
- The existing pings / callouts toggles are kept and join the same bar.
- An "N / M messages" counter permanently shows what the filter hides.

Pure helper `filterMessages(messages, {q, players, targets, types})` in `api.ts`, tested with vitest:
case, accents, empty filter = everything, player × target combination, message without text.

## Part C — Game context in the feed

Three additions, all fed by `/api/matches/{id}/replay2d`, enabled by a
"context" toggle (collapsed by default):

1. **Message sent while dead**: discreet mark on the line. Exact source —
   `heroes[].life: Interval[]` (`web/src/replay2d.ts:9`), not a death-proximity heuristic.
2. **Events interleaved** in the feed: deaths (`deaths[]`) and objectives (`objectives[]`), as
   separator lines typographically distinct from the messages. This is what gives the feed its
   meaning ("he flames after a triple kill").
3. **Chat density per minute**: a band aligned on the time axis of the existing level chart
   (`LevelChart`), which also serves as navigation — clicking a minute scrolls the feed.

Non-goal: no tone, toxicity or sentiment analysis. The feed shows, it does not judge.

## Part D — Cross-match chat search

Search for a phrase across the whole archive (~3,300 matches). It is the only part that touches the database and
the API.

### Data model — migration `0010_match_messages.sql`

Derived **read** table, not a new source of truth:

```sql
CREATE TABLE match_messages (
    match_id    BIGINT NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
    seq         INT    NOT NULL,          -- index in match.data->'messages'
    toon_handle TEXT,
    team        INT,
    recipient   INT,
    loop        INT,
    time_s      DOUBLE PRECISION,
    text        TEXT   NOT NULL,
    PRIMARY KEY (match_id, seq)
);
CREATE INDEX match_messages_trgm_idx ON match_messages USING GIN (lower(text) gin_trgm_ops);
```

- **Chat only** (`type = 0`): pings and callouts have no text to search.
- **Trigrams rather than `tsvector`**: chat is short, bilingual, misspelt and full of
  player names — tolerant substring search serves better than lemmatisation, and avoids having
  to choose a language configuration. Requires the `pg_trgm` extension (created by the migration).
- **Backfill in the migration itself**, via `jsonb_to_recordset` on `matches.data->'messages'`:
  no reprocess of the 3,300 replays is needed, the data is already in the database.
- `project_match` feeds the table on every projection; idempotence follows that of matches (the
  cascading `DELETE FROM matches`, `project.rs:104`).

### API — `GET /api/chat`

`q` (required, ≥ 2 characters), `player`, `mode`, `from`/`to`, `limit`/`offset`. Returns the
messages with their match, the resolved author and the immediate context (previous and next message),
sorted by descending game date. **Budget: p95 < 100 ms** (the project's API contract), measured
on the real archive.

### Front-end — `/chat` page

Results grouped by match, highlighted excerpt, click → `/match/:id` **anchored on the message**
(`#msg-<loop>`, an anchor also set by part B for sharing a specific message).

## Delivery order

B (pure front-end, zero risk) → A1 → A2 *if the frame-of-reference check passes* → C → D. Each part is
deliverable and verifiable on its own.

## Acceptance criteria

| Part | Measurable criterion |
|---|---|
| A1 | On a real replay, clicking 3 messages at distinct times positions the viewer within ±0.5 s; checked in the browser |
| A2 | The pings of a real replay land within the map frame and on plausible areas — otherwise the part is dropped and the reason written here |
| B | `npm test --prefix web` green on `filterMessages`; searching "gg" on a real match only returns lines containing "gg" |
| C | A message whose author is dead is marked, and the matching `life` interval confirms it in the data; the feed stays smooth on the longest replay of the corpus |
| D | Migration applied on the dev Postgres, **backfill verified** (`SELECT count(*)` consistent with the sum of type-0 messages in `matches.data`); `GET /api/chat?q=…` measured p95 < 100 ms on the archive |

## Documentation impact (rule no. 0)

| File | What changes |
|---|---|
| `docs/spec/08-frontend.md` | `/match/:id` row (chat linked to the viewer, filters, context) + new `/chat` route |
| `docs/spec/05-api.md` | `GET /api/chat` |
| `docs/spec/06-modele-donnees.md` | `match_messages` table, `pg_trgm` extension, migration `0010` |
| `docs/spec/01-architecture.md` | measured p95 of `/api/chat` |

## Open question for the operator

Observer messages remain discarded (hots-parser parity). You play custom
games with observers — do you want us to investigate lifting this filter in a
separate spec, or is it of no interest to you?
