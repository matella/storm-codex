# Manual play sessions

Product scope approved by the operator in chat: start, end, and rename a manual play session; correct a match assignment; keep automatic grouping; defer historical split/merge. This is a follow-up to the session review design, not a replacement for its scoring or baseline.

## Behavior

- The default Session dashboard shows **Start session** or **End session** and an indicator for the current manual session. One manual session can be open at a time for the local operator. Starting creates an empty session immediately, which remains open across restarts. Ending closes it without deleting any content. Give sessions a default display label and allow renaming; the optional stored name is trimmed and limited to 80 characters. Mutations display failures without pretending the change succeeded.
- Assign by `played_at` (game start), not replay ingestion. The manual window is start-inclusive and end-exclusive; a late replay starting within it belongs to it even after it ends. An explicit per-match override wins; the UI can move an eligible match to another manual session or explicitly use automatic grouping. This only updates grouping metadata, never the associated replay, match or stats. Prevent duplicate membership. Unassigned eligible matches keep the existing strictly-greater-than-three-hours grouping. Only dated operator matches are eligible.
- Session IDs for existing automatic groups remain their first match ID; manual IDs must be stable, unambiguous in the existing integer URL/API, and stay stable after rename or reassignment. Existing automatic links continue to resolve when their original first game is still a session anchor. The default dashboard prefers the open manual session (including empty); historical navigation includes manual and automatic sessions. Allow managing manual sessions from the default operator view. Account filters remain read-only and never show another player's games.
- Keep aggregate and per-match review math unchanged for identical membership. A match moved from before the session must not count in that session's baseline. Empty manual sessions have no games or numeric comparisons; do not present zero-denominator stats as a performance result. The dashboard and server-backed OBS recap read the same membership. Other OBS overlays keep their existing behavior in v1.

## Persistence and API

- Add a new migration for manual session windows, titles, and match-assignment overrides. Enforce only one open window in the database (including concurrent starts). Explicit automatic grouping is distinct from clearing an override. Preserve explicit assignments across reprocessing for the same replay fingerprint, even when the match ID changes. Use additive changes and no new service or dependency.
- Keep existing session GET routes and fields compatible. Add metadata needed to distinguish open/manual sessions and display names. Reuse existing admin authorization for writes; local standalone works without a new login, and a configured hosted admin token still gates mutations. Return a conflict if a second session is already running and a not-found or validation error for invalid/ineligible match or target IDs. Update the living server, data-model, API, and frontend contracts with the exact routes and shapes alongside the implementation.

## Acceptance

- Server tests: open empty session, persistence/readback, duplicate start, end, title validation, exact interval edges, late ingestion, move between manual sessions and forced automatic grouping, one membership, baseline exclusion, automatic 3 h boundary, unauthorized mutation, and operator perspective filtering. Use disposable PostgreSQL.
- Browser: start from the dashboard, reload, rename, move a match from a session, end, revisit history, and see truthful error states. Check keyboard interaction and test the release bundle. Keep existing Brotli/gzip size limits and record affected GET latency against a representative fixture if available.
- Out of scope: historical split and merge, deleting sessions or matches, player accounts, session sync to external services, changing the three-hour policy, and changing scoring or styling the rest of the dashboard.

## Implementation notes (2026-11-22)

- Status: implemented on `feat/manual-play-sessions`, not merged. Migration `0024_manual_play_sessions.sql`; exact routes and shapes live in `docs/spec/05-api.md`.
- Manual IDs are the negated `manual_sessions.id` (negative), so they cannot collide with positive automatic anchors. `/api/sessions` joins the guarded write prefixes in `auth.rs`.
- Deviations/additions: `GET /api/sessions/manual` (assignment targets, includes empty windows); `prefer_open=true` and `automatic_only=true` query flags (the latter keeps the stream-pool OBS source on its legacy review); assignments accept `{kind:"window"}` to clear an override.
- Server-backed OBS recap reads the preferred (open) session through a new `recap` feed; other OBS sources are unchanged.
- Verification: see `docs/STATUS.md`. Latency was only smoke-measured on an 8-game fixture; no representative-corpus p95 yet.
