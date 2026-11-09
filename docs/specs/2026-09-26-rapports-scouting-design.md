# Scouting reports

> storm-codex spec. Need expressed by the operator on 2026-09-26.
> **Approved by the operator on 2026-09-26** ("You can go"). Part A passed (drafts confirmed).
> Plan: `docs/plans/2026-09-26-rapports-scouting.md`.

## Goal

Prepare a match against an opposing team from **its replays**: the operator drops a
list of replays, storm-codex derives a set of quantified facts from them (draft, maps, hero pool,
how the games unfold), wraps them in a **pack** meant for an LLM (ChatGPT, Claude or a local
model), then **re-imports the LLM's analysis** in an imposed format to display it cleanly, each
claim backed by the figures that justify it.

Each report is a durable object: free editable title, timestamp, list of reports.

## Frame set by the operator (2026-09-26)

| Point | Decision |
|---|---|
| Use | **scouting 99 % of the time** (analysis of an opponent, not a review of one's own team) |
| Type of games | **all in custom games** (`Custom` mode, tournament draft) |
| Origin of the replays | the opponent's games **against other teams, and sometimes against us** |
| Users | **the operator alone** — no sharing, no public link |
| Analyses | **a single one per report**; re-importing **overwrites** the analysis in place |
| Content of the analysis (2026-09-26, after v1) | **the draft only, map by map**: which maps to pick or avoid, and for each map what to ban, what to pick, what they will probably play, what to plan for; plus a plan valid on any map. No more player notes or general game plan → `format_version: 2` |
| Roster rule | the target side **always has at least 3 players from the main team**, barring exceptions; for the exceptions, the operator can designate an **anchor player** |
| LLM step | **manual** (copy-paste or file): the operator has no OpenAI/Anthropic API — no LLM call from the app |
| Player names | **included** in the pack, with no option to remove them (they are only nicknames) |
| Language of the analysis | **English** (like the UI) |
| Link with `teams` | **no** — a report's roster lives in the report |

## What exists (state as of 2026-09-26)

| Element | Where | What it is used for here |
|---|---|---|
| Draft of custom games | `crates/storm-stats/src/process.rs:2239` | `match.bans`, `match.picks` (+ `first`), `players.*.turn` — the `Custom` mode is in the list of drafted modes (`process.rs:2246-2255`) |
| Upload + parse | `crates/storm-codex-server/src/upload.rs` | archive → parse → projection pipeline, `game_fingerprint` (`upload.rs:22`) |
| Write auth | `crates/storm-codex-server/src/auth.rs` | `is_admin`: `ADMIN_TOKEN` Bearer, open mode if absent |
| Draft simulator | `/draft`, `draft_live` | possible bridge (part F) |
| CSV export | `GET /api/matches.csv` | export precedent, **not reused** (see decision 1) |

## Decisions made

1. **Scouting replays do not go into `matches`.** They are mostly games of
   other players: projected into `matches`/`match_players`, they would pollute all the personal
   stats (`/api/heroes`, `/api/players`, `/api/maps`, `/api/synergies`, dashboard, lobby…).
   A `source` flag on `matches` would require filtering the forty or so queries that read
   these tables (`read.rs`: 24, `lobby/enrich.rs`: 6, `project.rs`: 6…) — a single one forgotten and the
   pollution is silent. We therefore store in **dedicated tables**: zero change to the
   existing queries, isolation guaranteed by construction.
   An opposing game **against us** can thus exist in both places (in `matches` via
   `client-rs`, and in the report): this is intended, the two uses are independent.
2. **No change to storm-stats or `PARSER_VERSION`.** All facts are computed from
   the `match`/`players` object that storm-stats already produces. No parity diff to redo.
3. **Facts are computed in Rust, not by the LLM.** An LLM computes rates poorly over
   hundreds of rows and readily invents figures; it therefore receives already-computed
   aggregates, each with its **sample size `n`**, and it **interprets**. The computation is a
   **pure** module (zero I/O) `crates/storm-codex-server/src/scouting/facts.rs`, unit-tested, like
   the draft engine.
4. **Each fact has a stable identifier** (`map.cursed_hollow.record`, `p2.hero.johanna`…) and
   **each claim of the analysis must cite the identifiers** it rests on. On import, the
   identifiers are resolved: the UI shows the real value and its `n` next to the claim, and
   **flags** any claim without evidence or citing a non-existent identifier. This is the safeguard
   against hallucination on samples of 10–20 games.
5. **Facts are a stored snapshot**, recomputed at each change of the report (replay added/removed,
   roster modified) and versioned (`facts_version`). Reads re-decode nothing and
   do not reload 30 `match` objects: they serve the snapshot.
6. **Players are designated `p1`…`pN` in the pack**, not by their BattleTag: short
   identifiers that the LLM copies without error, and that the UI translates back. The names always appear in
   a mapping table of the pack (operator decision: they are only nicknames).
7. **Write routes protected** like the existing management (`is_admin`, 🔒 if `ADMIN_TOKEN`).

## Part A — Prior check: the draft of custom games (entry gate)

The code declares `Custom` as a drafted mode, but nothing proves that the bans and pick order
come out right on **real tournament games**. First task, before everything else: decode
3 replays of custom games with draft and compare `bans`, `picks.first` and the pick order with
what is seen when replaying the game in the HotS client (operator confirmation).

If it is wrong or incomplete: the gap is documented here, and the "Draft" part of the facts is removed
from v1 (maps, hero pool and game flow remain deliverable) — a storm-stats fix
would be a parity divergence to investigate in `03-storm-stats.md`, separate spec.

### Decoding result (2026-09-26) — **confirmed by the operator** (part A passed)

3 replays provided by the operator (a series from 2026-07-22, build 97605, same 10 players; anchor
a player of the scouted team): Tomb of the Spider Queen, Alterac Pass, Braxis Holdout. Decoded without error;
`mode = -1` = `Custom` in `constants.json` (expected value, and the draft is indeed extracted).
Internal consistency checked: 3 bans per team (2 + 1), 5 picks per team, no banned hero
is picked in the same game, no duplicate.

What is **read** from the replay and what is **derived** — to keep in mind for the facts:

| Data | Origin |
|---|---|
| Heroes banned by each team, and phase (first 2 / 1 in the middle) | read (attributes 4023/4025/4043 and 4028/4030/4045) |
| Ban codes → hero name | `data/attr.json` (`Crus` → Johanna…) — **the pack must resolve the codes** |
| Team with first pick, pick order within a team | read |
| Global interleaving (ban A, ban B, ban A, ban B, pick A, B B, A A, ban B, ban A, B B, A A, B) | **derived**: standard order hard-coded in the parser (`process.rs:2343-2378`), not read |

Consequence: the draft facts speak of "first-phase / mid-draft bans" and of "pick order
within the team", never of a global rank that would only be a hypothesis.

## Part B — Storage, replay drop, target team detection

### Data model — migration `NNNN_scouting.sql`

Number = next free one at implementation time (`0010` is reserved by the "match
chat" spec if it goes first).

```sql
CREATE TABLE scouting_reports (
    id                   BIGSERIAL PRIMARY KEY,
    title                TEXT        NOT NULL,
    target_name          TEXT,                       -- free name of the scouted team
    roster               JSONB       NOT NULL DEFAULT '[]',  -- toon_handles confirmed by the operator
    anchors              JSONB       NOT NULL DEFAULT '[]',  -- anchor toon_handles (exceptions)
    facts                JSONB,                      -- snapshot of the facts (part C)
    facts_version        INT         NOT NULL DEFAULT 0,
    analysis             JSONB,                      -- validated imported analysis (part E), overwritable
    analysis_facts_version INT,                      -- facts_version at import time
    analysis_model       TEXT,                       -- declared by the LLM or entered, informative
    analysis_imported_at TIMESTAMPTZ,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE scouting_games (
    id             BIGSERIAL PRIMARY KEY,
    report_id      BIGINT      NOT NULL REFERENCES scouting_reports(id) ON DELETE CASCADE,
    fingerprint    TEXT        NOT NULL,             -- game_fingerprint (upload.rs:22)
    file_sha256    TEXT        NOT NULL,
    archived_path  TEXT        NOT NULL,
    filename       TEXT,
    played_at      TIMESTAMPTZ,
    map            TEXT,
    build          INT,
    target_team    INT,                              -- 0/1; NULL = target team not determined
    target_source  TEXT,                             -- 'roster' | 'anchor' | 'manual'; NULL if not determined
    data           JSONB       NOT NULL,             -- full storm-stats output {match, players}
    parser_version INT         NOT NULL,
    UNIQUE (report_id, fingerprint)
);
CREATE INDEX scouting_games_report_idx ON scouting_games (report_id);
```

(The applied migration file keeps its original French comments; it is never edited.)

- **Accepted implementation deviation (2026-09-26)**: `scouting_games` also carries `summary JSONB`
  (compact summary: teams, heroes, bans, picks, stats, levels, objectives), and `scouting_reports`
  `GENERATED ALWAYS AS IDENTITY` ids (convention of recent migrations). Recomputing the
  facts only reads the summaries, not N complete `match` objects; `data` remains the complete
  projection. `facts` stores the snapshot `{facts, detection, roster, roster_auto}`.
- **Raw replay archived** under `ARCHIVE_DIR/scouting/<sha256>.StormReplay` (3-tier rule:
  the raw file remains the source of truth).
- **Re-process**: `POST /api/admin/reprocess` also covers `scouting_games` (same
  `parser_version` criterion), then recomputes the facts of the affected reports. Idempotent.
- The same replay can belong to two reports (uniqueness per `(report_id, fingerprint)`).

### Target team detection

The games are the opponent's against various teams, sometimes against us:

Operator's rule: the target side **always has at least 3 players from the main
team**, barring exceptions — and for the exceptions, an **anchor player**.

1. **Candidates**: the `toon_handle`s present in at least 50 % of the report's games. If an
   anchor is already defined, detection starts from it: candidates = players on the same side as the anchor
   in at least 50 % of the anchor's games (more robust when the batch contains many
   games against the same team, us included).
2. **Target side of a game**, in this order:
   1. the side that contains **at least 3 players of the confirmed roster** → `target_source = 'roster'`;
   2. otherwise, the side that contains **an anchor player** → `'anchor'` (the exception: reshuffled line-up);
   3. otherwise, **manual choice** by the operator in the Games tab → `'manual'`.

   As long as no rule decides: `target_team = NULL`, game shown as "team not found" and
   **excluded from the facts**. An anchor present on both sides (impossible in game, so corrupted
   data) → not determined.
3. The operator **confirms or corrects** the roster (ticks/unticks, adds a player) and designates
   **0, 1 or several anchors**; any modification reruns step 2 (manual choices are
   kept) then the facts computation.
4. Players on the target side outside the confirmed roster: counted as **substitutes** (appear in
   the player facts with their number of games).

Player identifier = `toon_handle`, never the displayed name (which changes and looks alike from one player
to another).

### Drop

The front-end sends the files **one by one** (progress bar, status per file: added /
duplicate / parse failure / team not found) via `POST /api/scouting/{id}/replays` — body =
raw bytes, `X-Filename` header, same contract as `/api/upload`. Parse on the same pool as
the upload; existing parse budget (< 150 ms/replay).

## Part C — Computed facts

v1 catalogue. **The exact list of source fields is fixed in the plan**, after checking their
presence in the `match` object of custom games; a fact whose source is missing is removed, not
approximated.

| Family | Facts (indicative id) |
|---|---|
| Overview | games, W/L record, period covered, builds/patches, average duration (`overview.*`) |
| Maps | per map: games, W/L, winrate + **95 % Wilson interval** (`map.<map>.record`) |
| Players | per player `pN`: games, heroes played (games, wins), role breakdown, average KDA, hero damage/min, siege/min, healing/min, deaths/game, share of the team's damage (`pN.*`) |
| Hero × map | heroes picked by the target on each map (`map.<map>.picks`) |
| Draft | first pick rate; bans made by the target (overall, per map, per phase); bans received; heroes taken first pick; role taken first; last picks; **heroes played against them** with their record in those games (`draft.faced.*`) (`draft.*`) |
| Picks per map | heroes picked by the target on each map **with the player `pN` who played it** (`map.<map>.pick.*`) |
| Game flow | winrate by first to 10, first boss, first objective; average level gap at 10/15/20 min; winrate per duration bracket; comebacks and games lost while ahead (`flow.*`) |

Rules:
- **Each fact carries its `n`.** A fact with `n < 3` is marked "small sample" in the
  pack (the prompt forbids drawing a firm conclusion from it) and greyed out in the UI.
- Rates presented as `k/n` **and** as a percentage, never as a percentage alone.
- Pure module `scouting/facts.rs`: input = games + roster, output = serialisable `Facts`.

## Part D — The LLM pack

Two outputs, same content:

1. **`GET /api/scouting/{id}/pack.md`** (and a "Copy pack" button) — **main format**,
   readable by all LLMs including local ones:
   - the scouting analysis **prompt**, **in English**, answer required in English (role, goal: prepare draft and game plan against
     this team; rules: cite the ids, assert nothing without a fact, flag small
     samples);
   - the **definition** of each family of facts;
   - the `pN` → name table;
   - the **facts**, as Markdown tables, each row with its id;
   - the **imposed response format** (JSON Schema, part E) and a minimal example;
   - `report_id` and `facts_version`, to be copied into the response.
2. **`GET /api/scouting/{id}/pack.xlsx`** — one sheet per family of facts + a Games sheet
   (1 row per game: date, map, opponent of the day, result, duration, bans, compositions)
   + a README sheet. For human reading and for LLMs that read Excel.
   Writing library chosen in the plan, **docs checked via Context7**.

The prompt lives in a versioned file of the repo (`crates/storm-codex-server/src/scouting/prompt.md`,
embedded), not hard-coded.

## Part E — Importing the analysis

### Imposed format (`format_version: 2` — map-by-map draft plan)

v1 (player notes, tendencies, general game plan) was replaced on 2026-09-26 at the
operator's request: they only want the draft, map by map. A v1 response is refused on import
("copy the pack again").

```json
{
  "format_version": 2,
  "report_id": 12,
  "facts_version": 3,
  "model": "claude-opus-5-5",
  "summary": "… the team's draft identity (3-5 sentences)",
  "map_choice": {
    "pick":  [{ "map": "…", "why": "…", "evidence": ["map.x.record"] }],
    "avoid": [{ "map": "…", "why": "…", "evidence": ["…"] }]
  },
  "maps": [{
    "map": "Braxis Holdout", "confidence": "low|medium|high",
    "overview": "what we expect from them on this map", "evidence": ["…"],
    "bans":        [{ "hero": "…", "phase": "first|mid", "why": "…", "evidence": ["…"] }],
    "picks":       [{ "hero": "…", "why": "…", "evidence": ["…"] }],
    "their_picks": [{ "hero": "…", "player": "p3", "why": "…", "evidence": ["…"] }],
    "considerations": [{ "point": "…", "evidence": ["…"] }]
  }],
  "general": { "bans": [], "picks": [], "considerations": [] }
}
```

One `maps` entry per map present in the facts; `general` applies to any map, including
those absent from the replays. Each item carries its `evidence` (checked on import).

The JSON Schema is a file of the repo, embedded in the pack and used for validation.

### Import — `PUT /api/scouting/{id}/analysis`

Body = **raw text** of the LLM's response (pasted or dropped file). The server:

1. **Extracts** the JSON: ```` ```json ```` block if there is one, otherwise the first balanced JSON object in the
   text (local models often surround it with prose).
2. **Validates** against the v1 schema — tolerant: unknown fields ignored, absent sections
   accepted. Broken JSON or unknown `format_version` → refused with the exact error, nothing is
   overwritten.
3. **Resolves the evidence**: each cited id is looked up in the current facts; a claim without
   evidence or with an unknown id → marked `unsupported` (kept, flagged).
4. **Consistency check**: different `report_id` → refused; earlier `facts_version` → accepted
   with the warning "analysis made on an earlier state of the report".
5. **Overwrites** the existing analysis (operator decision) — the UI asks for confirmation before sending
   when an analysis already exists.

Response: the validated analysis + an import report (number of claims, number of `unsupported`,
warnings). `DELETE /api/scouting/{id}/analysis` removes the analysis.

### Status of a report (derived, not stored)

`empty` (no game) → `ready` (facts computed, no analysis) → `analyzed` → `stale` (analysis
imported on an earlier `facts_version`: replays have been added/removed since).

## Interface

UI language: English (`08-frontend.md` convention).

### `/scouting` — list of reports

Cards sorted by descending creation date: **title**, target team, creation date,
number of games, period/patches covered, **status badge**, analysis import date.
"New report" button (title + team name → opens the report).

### `/scouting/:id` — a report

- **Header**: title **editable in place**, target team, creation / update dates,
  status, actions: *Copy pack*, *Download .md*, *Download .xlsx*, *Import analysis*.
- **Tabs**:
  - **Overview** — draft summary + **map choice** (to pick / to avoid);
    overall facts.
  - **Players** — one card per player (`pN` + name + avatar): hero pool, roles, key stats,
    threat level and notes from the analysis.
  - **Draft** — the analysis's "any map" plan, then bans, first/last picks, heroes played against
    them with their record.
  - **Maps** — **the draft plan**: one card per map (record + interval, what they picked and
    by whom, their bans, the bans received), then "We ban" (with phase), "We pick", "They will
    likely pick" (with the player), "Consider", and the confidence level. The maps that
    the analysis covers without them being in the replays also appear ("not in these
    replays").
  - **Games** — the list of the report's replays, drop zone, status per file, target side
    detected and **by which rule** (roster / anchor / manual), manual choice of side for an
    undetermined game, removal of a replay.
  - **Roster** — detected candidates, checkboxes, substitutes, **designation of anchors**.
- **Evidence**: each claim of the analysis carries **chips** with the real value and `k/n`
  (hover = detail of the fact); an `unsupported` claim is visibly marked.
- **Import**: dialog with a paste area + file drop, preview of the import report before
  confirmation, overwrite confirmation if an analysis exists.
- The report is **useful without an analysis**: all tabs show the facts alone.

## API (summary)

| Route | Role |
|---|---|
| `GET /api/scouting` | list (without `facts` or `analysis`) |
| `POST /api/scouting` 🔒 | create `{title, target_name}` |
| `GET /api/scouting/{id}` | complete report: meta, games (without `data`), roster candidates, facts, analysis with resolved evidence, status |
| `PATCH /api/scouting/{id}` 🔒 | `title`, `target_name`, `roster` (→ recompute) |
| `DELETE /api/scouting/{id}` 🔒 | delete (games cascade; archived raw files remain) |
| `POST /api/scouting/{id}/replays` 🔒 | add a replay (raw bytes, `X-Filename`) |
| `DELETE /api/scouting/{id}/replays/{gid}` 🔒 | remove a replay (→ recompute) |
| `GET /api/scouting/{id}/pack.md` · `/pack.xlsx` | LLM pack |
| `PUT /api/scouting/{id}/analysis` 🔒 | import (overwrites) |
| `DELETE /api/scouting/{id}/analysis` 🔒 | remove the analysis |

**Budget**: `GET /api/scouting/{id}` p95 < 100 ms (API contract) on a 30-game report —
held by the facts snapshot (decision 5), measured.

## Part G — Shareable HTML export (operator request, 2026-09-27)

**Export HTML** button on a report's page: downloads **a single standalone `.html` file**
that the operator sends to their teammates (Discord, mail). Requirement: "highly visual".

- **Standalone**: hero portraits and minimaps embedded as `data:` URIs (shrunk in the
  browser: portraits 128 px, minimaps 900 px, WebP); no dependency on the app or the box.
  Only the fonts come from Google Fonts, with a system fallback if offline.
- **Generated in the browser** (`web/src/reportExport.ts` loads the images; `web/src/reportHtml.ts`
  renders — a **pure** function, tested in vitest). No new server route.
- **Security**: any text coming from the LLM or the replays is escaped (the file circulates outside the app).
- **Art direction**: a "scouting dossier" in the language of the HotS draft screen.
  Cover (team name in large type, record with W/L pips, first pick, small-sample
  warning, roster medallions), draft identity (summary + bans/openings as
  portraits), map choice (tiles on the minimap), **a draft board per map** (minimap
  header, confidence gauge, columns "We ban" — portraits struck through in red with the phase —,
  "We pick" — green glow —, "They will likely pick" — orange glow + player —, "Consider", then
  what they played/banned on the map), "any map" plan, player cards (pool as bars),
  heroes faced (those who beat them highlighted), game log. Each recommendation
  keeps its evidence (label + value); "⚠ unverified" if none. Map navigation at the top,
  responsive, clean printing, animations respecting `prefers-reduced-motion`.
- Works without an imported analysis (facts alone) and without portraits (initial medallions on
  the universe colour ring).

### App page in the same style (operator request, 2026-09-27)

"The same visuals in the site, without touching the export": the `/scouting` and
`/scouting/:id` pages adopt the export's language (cover, draft boards per map, struck-through /
glowing portraits, compact evidence). The export is **not modified**: the page only reuses
its pure helpers (`compactEvidence`, `humanize`) and its universe colour table. The export's styles
style `body`/`h1`/`main`: they are **ported** into a stylesheet scoped to `.sd`, not
injected. The tabs disappear in favour of a single page with a section bar; drop,
manual side and roster become the last two sections.

## Part F — Later, outside v1

- **Match page for scouting replays** (reuse `MatchDetail` and the 2D viewer
  on a `/api/scouting/{id}/games/{gid}` source with the same shape as `/api/matches/{id}`).
- **Bridge to the draft simulator**: load the report's tendencies and recommended bans
  into `/draft` during a real draft.

**Non-goal**: no LLM call from the app (the operator has no OpenAI/Anthropic API).
The LLM step is manual: copy the pack, paste it into ChatGPT/Claude/a local model, paste the
response back. Hence the importance of the "Copy pack" button and of a tolerant import.

## Delivery order

A (entry gate) → B → C → D (`.md` first, `.xlsx` next) → E + UI. Each part is
verifiable on its own; B–C are useful without an LLM.

## Acceptance criteria

| Part | Measurable criterion |
|---|---|
| A | 3 real replays of custom games with draft: bans, first pick and pick order identical to what the HotS client shows (operator confirmation) — otherwise the gap is written here and the Draft part removed |
| B | Migration applied on the dev Postgres; on a real batch of a team's replays, the detected roster matches the team (operator confirmation); **isolation proven**: responses of `/api/heroes`, `/api/maps`, `/api/players/{toon}` identical before/after adding 20 scouting replays (integration test); idempotent reprocess on `scouting_games` |
| C | Unit tests of `scouting/facts.rs`: records, Wilson, exclusion of games without a target side, substitutes, `n` on each fact; tests of side detection: ≥ 3 from the roster, fallback on the anchor, manual choice kept after a roster change, anchor on both sides → not determined |
| D | `.md` pack of a real report given to Claude **and** to ChatGPT: both return a response that the import accepts; `.xlsx` opened without error in Excel |
| E | Tests of the extraction (fenced block, surrounding prose, broken JSON → refused without overwriting, wrong `report_id` → refused) and of evidence resolution (unknown id → `unsupported`); `GET /api/scouting/{id}` measured p95 < 100 ms on 30 games; browser check on a real report, analysis imported then re-imported (overwrite) |

## Documentation impact (rule no. 0)

| File | What changes |
|---|---|
| `docs/spec/05-api.md` | "Scouting" section (routes above) |
| `docs/spec/06-modele-donnees.md` | tables `scouting_reports`, `scouting_games`, migration |
| `docs/spec/08-frontend.md` | routes `/scouting`, `/scouting/:id` |
| `docs/spec/04-serveur.md` | archiving `ARCHIVE_DIR/scouting/`, extended reprocess |
| `docs/spec/01-architecture.md` | measured p95 of `GET /api/scouting/{id}` |

## Questions settled (2026-09-26)

1. **Roster**: at least 3 players from the main team, otherwise an anchor player (and manual choice as a
   last resort) — integrated into part B.
2. **Link with `teams`**: no.
3. **Names in the pack**: always included.
4. **Language of the analysis**: English.
