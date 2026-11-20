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
