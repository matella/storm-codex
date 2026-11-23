# Streamer widgets, round 1

Date: 2026-11-20 · Status: **APPROVED** by the operator in chat (2026-11-20: all 7 widgets below, local-DB
persistence for the goal tracker, no Twitch/Discord/accounts).

## Goal

Add seven small, local-data-only OBS browser sources that give streamers more to put on screen, with no new
account, no third-party request, and (for six of the seven) no backend change — they read data the server
already computes. Scope is explicitly **not** Twitch/Discord/follower-alert style widgets; everything here
comes from the operator's own replay archive.

## Widgets

1. **Win/loss streak badge.** `web-rs/sources/src/sessions.rs`'s `Ticker` already computes a `streak: i32`
   field for its pill but only shows it as plain text. Add a milestone flash (CSS animation, no new request)
   when `streak.abs()` crosses 3/5/7/10: a brief glow/scale pulse and a "X-game win/loss streak" caption,
   then settle back to the existing quiet pill. No new route, no backend change.
2. **Session recap card.** New source `/recap`, reusing `GET /api/sessions?limit=1` → `GET
   /api/sessions/{id}` (already returns `Review`: win rate vs baseline, streaks, per-hero/per-map rows,
   best/worst game cards). Render a condensed version: record, streak, best hero this session, standout
   game. Auto-refreshes on the existing `matches.updated` live-bus event, same pattern as `/widget`. No
   backend change.
3. **Personal-record flash.** New source `/record-flash`. On every `matches.updated` event, re-fetch `GET
   /api/records?mine=true&limit=1` (already exists, `insights::api::records`) and compare the top row's
   match id to the newest match id from the live feed; if they match, show "New record: <stat> (<value>)"
   for ~8s then hide. No backend change (the records endpoint already supports `mine`/`limit`).
4. **"Played this stream" hero pool.** New source `/stream-pool`, reusing the same `GET
   /api/sessions/{id}` `Review.heroes: Vec<Row>` the Session page already renders (per-hero W–L for the
   current session = the operator's own 3h-gap session split). Render as a compact bar list (hero portrait,
   games, W–L). No backend change.
5. **Nemesis/grudge-match badge.** New source `/nemesis`. Reads `GET /api/live` for the current lobby's
   enemy toons (already available to the live/draft overlay); for each resolved enemy toon, call `GET
   /api/matches?against=<toon>&limit=1` (existing filter, `read/filter.rs`) and read `X-Total-Count` plus the
   win/loss tally already exposed via the archive's per-row `perspective_toon` result. Flag an opponent as a
   "grudge match" when the operator's record against them is ≤ 35% over ≥ 3 games, and show their name badge
   during Draft/loading. No backend change; several small client requests (≤ 5 enemies), debounced to once
   per lobby.
6. **Patch-day banner.** New source `/patch-banner`. Reads `GET /api/patches?limit=1` (existing); compares
   the latest patch id/date against a `localStorage` "last seen" value (same pattern as the existing
   `standalone_welcome_decided` key) and shows a banner for up to 7 real-world days after a patch the
   operator hasn't acknowledged, then hides itself (no dismiss button needed, OBS sources are not clicked).
   No backend change.
7. **Daily goal tracker.** The one widget needing new state: the operator sets a single active goal (type:
   wins today / games played today; target: a small integer), and a new source `/goal` shows live progress
   ("2 / 3 wins today"), resetting at local midnight.
   - **Backend (new, small):** one migration, table `app_goal` (single row, `kind text`, `target int`,
     `created_at`), a `GET /api/admin/goal` / `PUT /api/admin/goal` pair (admin-token gated, same contract
     as other Admin writes) to set/clear it, and `GET /api/goal/progress` (no auth, read-only, like other OBS
     endpoints) returning `{kind, target, done, reset_at}` computed from the day's matches for the operator's
     perspective. Progress is **not stored**, it's recomputed from the existing archive on each read — only
     the goal definition itself is persisted, in the local DB (operator decision 2026-11-20: DB-backed, so
     it's automatically covered by the existing Export/Import backup, consistent with other Admin settings).
   - **Admin UI:** one small block in `/admin` ("Daily goal"): kind selector, target number input, Save /
     Clear. Pure helpers in `core/goal.rs` (native tests), app side in `app/src/pages/admin.rs`.
   - **OBS source:** polls `/api/goal/progress` every 10s (same cadence as other sources' safety-net poll);
     renders nothing when no goal is set (empty state: a blank transparent frame, not a placeholder message,
     since OBS sources with no content should not draw a visible box).

## Out of scope

Twitch/Discord/follower/subscriber/chat/bits widgets, overlay builder/custom layout canvas (separate idea,
not part of this spec), new skins for these sources (ship with the existing Nexus Codex look only), any
widget needing data outside the operator's own archive (team/league-wide stats, opponent scouting reports
as an OBS source).

## Budgets and acceptance

- OBS `sources` bundle stays ≤ 300 kB gzip (currently 181,362 bytes; room for all seven, but measure and
  report the delta — if it gets close, prefer sharing markup/CSS over duplicating per-widget).
- No change to the `overlay` or `app` bundles except the small Admin "Daily goal" block in `app`.
- Existing OBS sources (`/ticker`, `/widget`, `/queue`, `/now-playing`) keep their current behaviour; the
  streak-flash change to `/ticker` must not alter its layout when no milestone is active.
- New endpoints (`/api/goal/progress`, `/api/admin/goal`) get server unit + endpoint tests; `core/goal.rs`
  gets native unit tests for the progress computation (midnight reset, kind switch, no-goal state).
- Each new source checked in headless Chromium against a copy of the box DB: milestone flash fires at the
  right streak lengths, record-flash fires only on an actual new record and clears after ~8s, hero pool
  matches the Session page's own per-hero rows for the same session, nemesis badge only shows for a real
  ≤35%/≥3-game opponent, patch banner respects the 7-day/localStorage rule, goal tracker resets at local
  midnight and renders nothing with no goal set.
- Perf: the nemesis badge's per-opponent requests must not exceed the existing API p95 budget
  (`docs/spec/01-architecture.md`); debounce so a 5v5 lobby does 5 requests once per lobby, not once per
  live-feed tick.

## Implementation notes (2026-11-20, branch `feat/streamer-widgets-recap`)

Scope of this branch: **widgets 1, 2, 4, 6 only** (streak flash, session recap, "played this stream" hero
pool, patch-day banner). Widgets 3, 5, 7 (personal-record flash, nemesis badge, daily goal tracker) are out of
scope for this branch and untouched.

- **`GET /api/sessions` / `GET /api/sessions/{id}` already existed** (merged earlier as part of the Local
  insights session-review feature, `docs/specs/2026-11-19-session-review.md`, consumed by the app's Session
  page) — this spec's premise that they're pre-existing read endpoints was correct, just not literally new
  for this feature. `/recap` and `/stream-pool` call them directly (`storm_web_core::review::{Review,
  SessionList, Row}`, already native-tested), which is strictly better than a client-side reimplementation:
  the server's `best`/`worst` picks are baseline-scored, not just "most recent MVP or best KDA" as an
  earlier draft of this branch computed client-side before this was found. No backend change either way.
- **Streak milestones**: `streak.abs()` crossing exactly 3, 5, 7 or 10 (not every game once past a
  threshold) triggers a 2x pulse animation (~1.2 s) plus a caption held for 4 s total, then the pill settles
  back with no residual class — verified in Chromium (class `ticker-pill flash flash-win`/`flash-loss` with
  a `.ticker-caption`, reverting to plain `ticker-pill` after the window). The milestone set and the glow
  color are an implementation choice (spec gave the thresholds but not timing/animation specifics).
- **Patch banner "acknowledgment"**: there is no dismiss button (OBS sources aren't clicked), so a display
  counts as the acknowledgment — the widget itself writes `localStorage.patch_banner_last_seen = <id>` the
  first time it shows a patch, so the same patch does not re-flash on a source reload within the 7-day
  window; a *new* patch id still shows even if the previous one was "seen". Verified: fresh/unseen patch
  visible, same patch on reload hidden, a patch older than 7 days hidden regardless of `last_seen`.
- **Recap "best hero this session"** = the most-played hero of `Review.heroes` (server-sorted by games
  descending already); "standout game" = `Review.best` (omitted, not a placeholder, when the server found no
  comparable best game, e.g. a single-game session).
- **Bundle size**: `sources` 181,362 → 208,614 bytes gzip (delta **+27,252 bytes**, well inside the 300 kB
  budget — room remains for widgets 3/5/7 in a later branch). `overlay` unchanged (217,951 bytes). `app`
  1,046,825 → 1,047,110 bytes brotli (delta **+285 bytes**: the shared `core` crate's `review` module was
  already linked into `app` for the Session page; no new app page, budget untouched at 1.25 MB).
- **Found, not fixed (out of scope)**: `storm_web_core::aggregates::PatchItem::community_only` is a plain
  `bool`, not `Option<bool>`; a `dim_patches` row whose `data->'communityOnly'` key is present but SQL `NULL`
  (not just absent) fails JSON deserialization of the whole `/api/patches` response, which would make
  `/patch-banner` (and the app's own Patches/`hero_changes` pages, which share the same struct) silently stop
  updating. Not observed on real refdata-ingested rows in production (the adapter always writes a boolean),
  only surfaced here with a hand-inserted test row in the throwaway DB copy; flagged for the operator rather
  than touched, since fixing it is a change to an existing, unrelated contract type outside this spec's
  approved scope.

## Implementation notes (items 3 and 5, 2026-11-20, branch `feat/streamer-widgets-flash`)

Only items 3 (personal-record flash) and 5 (nemesis/grudge-match badge) were implemented on this branch;
items 1, 2, 4, 6 and 7 are untouched. Two deviations from the literal wording above, both because the named
event/endpoint does not exist or does not carry the field described — the underlying behaviour (reuse an
existing read endpoint, no backend change, react to the existing live-bus) is unchanged:

- **"matches.updated event" → `match.parsed`.** The server only ever emits `match.parsed` on a new parse (see
  `upload.rs`); there is no `matches.updated` type. Both `/record-flash` and the rest of this spec's widgets that
  mention it reuse `frame::recent_matches`, the same hook `/ticker`/`/widget`/`/queue` already use, which refetches
  on `match.parsed`.
- **"`GET /api/live` for the current lobby's enemy toons" → `GET /api/lobby`.** `GET /api/live`'s view
  (`live/state.rs::view`) deliberately never serialises a toon handle, by design (fair-play scope governs
  *ability* visibility, not identity) — there is no toon field to read at any scope. `GET /api/lobby` (the
  loading-screen companion, `lobby::LobbyPlayerState`) is the endpoint that already resolves `toon_handle` per
  player, including opponents, for the operator's own build suggestions; it matches this item's own wording
  ("the **current lobby**'s enemy toons", "during **Draft/loading**") more literally than the live feed does.
  `/nemesis` reads it on the existing `lobby.detected`/`lobby.updated` events instead of `live.updated`.
- The win/loss tally per opponent is two `limit=1` requests against the existing `against` filter — one plain,
  one with `&result=win` — reading `X-Total-Count` on each (games, wins); no new aggregate endpoint.
- Debounce key: `storm_web_core::nemesis::lobby_signature` (`content_hash` + the sorted resolved enemy toons,
  empty until at least one opponent is resolved). The per-opponent lookups only run again when this signature
  changes, and only on `lobby.detected`/`lobby.updated` (not the existing 10 s safety-net poll, which is not even
  listened to), satisfying the "once per lobby, not once per live-feed tick" acceptance bullet with margin.
- Privacy: the badge shows only the opponent's name (as already recorded in the archive, the same field the
  app's "Played against" picker and Compare page already read) and the operator's own W–L against them — nothing
  about the opponent beyond that, consistent with "not leaking opponent identity info beyond what Draft/cast
  already expose". An opponent whose toon is not yet resolved (`toon_handle: null`) is silently excluded rather
  than guessed at.
- Measured (box DB copy, 3,503 matches, release build): `sources` bundle 199,574 bytes gzip / 165,517 bytes
  brotli (+18,212 bytes gzip over the 181,362-byte baseline); `overlay` and `app` unchanged. Checked in headless
  Chromium against the box DB copy: `/record-flash` shows nothing against the real newest match (no coincidental
  record), shows "NEW RECORD · Most kills · 25" when the newest match id is mocked to the real top-kills record's
  match id, and hides again after 8 s; `/nemesis` against a mocked `/api/lobby` roster (real toons from the
  corpus) correctly flagged a real 0/6 opponent, correctly did not flag a real 3/3 (100%) opponent, never queried
  the unresolved-toon opponent, and made no further request across a 10 s poll tick (debounce holds); with no
  active lobby (`204`), `/nemesis` renders nothing.

## Implementation notes (2026-11-20, branch `feat/streamer-widgets-goal`)

Only item **7 (daily goal tracker)** is implemented on this branch; items 1–6 (streak flash, session
recap, personal-record flash, stream pool, nemesis badge, patch-day banner) are not started. No
deviation from the approved shape of widget 7: single-row `app_goal` table (migration `0019`), the
admin-gated `GET`/`PUT /api/admin/goal` pair, the public read-only `GET /api/goal/progress` recomputed
on every read, the `/admin` "Daily goal" block (kind selector, target, Save/Clear), and the `/goal`
source polling every 10 s with a blank frame when no goal is set.

One implementation detail not spelled out in the spec text: "local midnight" is the **server's** local
time (`chrono::Local`, the box's configured timezone), not the viewer's browser timezone — the server
has no reliable way to know the OBS browser source's timezone, and the operator and the box are
expected to share one (home streaming setup). Pure day-boundary math lives in
`crates/storm-codex-server/src/goal.rs` (`day_bounds`, parameterised by `DateTime<FixedOffset>` so it is
native-tested without depending on the real clock); `web-rs/core/src/goal.rs` holds the client-side
vocabulary (`GoalKind`, `Progress`, `display_line`) shared between the Admin block and the `/goal`
source — the source trusts the server's `reset_at` rather than recomputing its own boundary.

Measured: `sources` bundle 185,111 bytes gzip (was 181,362; +3,749 bytes), well under the 300 kB cap;
`app` bundle 1,050,106 bytes Brotli (was 1,046,738; +2,996 bytes), under the 1.25 MB cap.
`GET /api/goal/progress` measured ~1.0 ms avg / 1.5 ms worst (25 warm loopback requests), far under the
100 ms API p95 budget — no change needed to `01-architecture.md`'s targets, only the new measured row.

Checks run: `cargo fmt`/`clippy --workspace --all-targets -D warnings` (server workspace) and the
`web-rs` fmt/clippy native + wasm32 sequence, all clean; 462 server workspace tests (throwaway Postgres
17, 0 failed) including 11 new `goal::` tests (midnight reset, kind switch, no-goal state, admin-token
gating, upsert/clear lifecycle, operator-perspective progress); 189 `web-rs` native tests (was 181, +8
`core::goal` tests); all three Trunk release builds passed their size budgets. Browser-checked against
a throwaway copy of the DB with a release server: goal set → `/goal` shows live progress matching real
`matches`/`match_players` rows; goal cleared → `/goal` renders a blank `<div></div>`; a win inserted with
`played_at = now() - 1 day` is correctly excluded from "today" (midnight-boundary proof against real
data, alongside the native injected-clock tests). Self-reviewed (no `delegate_task` tool available in
this session to dispatch an isolated `[Reviewer]` subagent): admin-token gating verified by test on both
new routes, `GET /api/goal/progress` confirmed deliberately public/read-only, all SQL parameterized
(`$1`/`$2`/`$3` binds, no `format!`/interpolation of user input), `kind`/`target` validated both
Rust-side (`GoalKind::parse`, `target > 0`) and by the DB `CHECK` constraints (defence in depth), no
`unwrap()`/`expect()` outside `#[cfg(test)]`. Full details in `docs/STATUS.md`.

## Bug fix (2026-11-23, branch `fix/widget-ux-polish`)

**Item 2 (session recap), standout-game award label**: `web-rs/sources/src/recap.rs`'s `recap_card`
rendered `ReviewGame::award` (the raw enum-like string stored on the match row, e.g.
`EndOfMatchAwardMostHealingBoolean`) directly instead of running it through
`storm_web_core::session::award_label` the way every other award-displaying view (`matchui.rs`'s
`AwardTag`, `card.rs`, `sessions.rs`'s `Ticker`/queue rows, `matches.rs`, `player.rs`) already does —
an oversight from when `/recap` was first built, not a regression. Fixed to call `award_label` and
render `"{icon} {label}"` (or `"👑 MVP"`), matching the existing convention. No contract change: `award`
was always `Option<String>` on the wire, this only touches how the client formats it.

**Items 3, 5, 6 (personal-record flash, nemesis badge, patch-day banner) showing nothing in the
overlay builder's live preview is expected, not a bug.** All three are condition-triggered by design
(spec text above): `/record-flash` only renders for ~8 s right after the newest game set a personal
best; `/nemesis` only renders while the live lobby contains an opponent the operator currently has a
≤35%-over-≥3-games record against; `/patch-banner` only renders for up to 7 real-world days after a
new patch and only until first shown once (`localStorage` "last seen"). The builder's preview iframe
(`?preview=1`) renders the real widget against the real local server, with no fixture/sample-state
override — so outside those exact trigger windows the canvas legitimately shows the same blank
`<div></div>` the OBS source itself would show at that moment. This does make these three widgets hard
to visually place/style blind in the builder; a follow-up (preview-only sample content, operator
decision needed) is tracked separately, out of scope for this bug-fix pass.

