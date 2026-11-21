# Standalone identity — confirmed accounts (2026-11-21)

## Problem

A standard HotS replay folder encodes a ToonHandle, and the server already derives "me" (`operator_toons`, used across matches/insights/lobby/goal reads) from the *currently configured* replay folders — read-only, computed live, never persisted. If a folder is removed, renamed, or the player plays on a second PC without that folder, every "me" view silently loses that account. There is also "Manual in-game names" (`operator_names`, free-text display names, matched by current name only — breaks on a rename, no stable id).

Operator ask: let a detected account be kept in "my identity" independent of whether its replay folder stays configured, keyed by the stable ToonHandle, not the display name.

## Decision

Add a third source of "me", alongside live folder detection and manual names: an explicit, persisted set of confirmed ToonHandles. `operator_toons()` becomes the union of (a) ToonHandles from currently effective folders and (b) persisted confirmed ToonHandles. Saving is explicit per account (a button), never automatic — a shared PC's detected-but-unconfirmed accounts are not silently added.

### Server
- New migration `0022_confirmed_identity.sql`: table `confirmed_identity(toon TEXT PRIMARY KEY, confirmed_at TIMESTAMPTZ NOT NULL DEFAULT now())`. Toon format already validated by `folder_toon` on the way in; store as given (same string the frontend already displays).
- `standalone::identity::operator_toons` (server, `crate::standalone::operator_toons`) becomes `async`, unions live folder toons with `SELECT toon FROM confirmed_identity`, dedups, sorted. All 7 call sites (`goal.rs`, `insights/api.rs` x3, `lobby/api.rs` x2, `manage.rs`, `read.rs` x5) already call it per-request; threading the DB read through is a mechanical await, not a redesign.
- `GET /api/standalone/identity` (`folder_identities`) additionally reports `confirmed: bool` per folder row (`toon` already known) so the UI can show current state. Keep the response read-only in shape, just one more field.
- New endpoints, admin-gated (same pattern as `PUT /api/admin/settings`): `POST /api/admin/identity/confirm { toon }` (idempotent upsert) and `DELETE /api/admin/identity/confirm/{toon}`. 404 if `toon` is empty; no format re-validation beyond non-empty (the server only ever sees toons it itself produced and displayed).

### Frontend
- `storm_web_core::standalone::FolderIdentity` gains `confirmed: bool`.
- `FolderIdentities` component (`web-rs/app/src/ui.rs`): per row with a known `toon`, a button toggles between "Add to My Identity" and "Added to My Identity ✓" (then becomes a small "Remove" affordance), calling the two new endpoints; optimistic state reverts on a non-2xx response with an inline error, matching the existing `Identity`/`save` pattern in `welcome.rs`.
- `use_owner_toons()` (`web-rs/app/src/ui.rs`) already derives from `folders`; since the server-returned `folders` already reflects confirmed accounts via the unioned `operator_toons` server-side... no — `use_owner_toons` derives toons straight from the `folders` list's own `toon` field (live-detected only), not from `operator_toons`. Keep that as is for the UI's own "my toons" memo (it's about *display*, e.g. highlighting "you" in a lobby render fed by `/api/standalone/identity`, which already includes every detected folder's toon) — the *server-side* `operator_toons()` union is what actually matters for filtering "my matches" server-side, independent of the UI memo. No web-rs/core change needed there beyond the new `confirmed` field.

## Out of scope
- No merge/unlink of a confirmed toon's match history into another toon (that's the existing, separate player-identity "suggestions" feature in `web-rs/app/src/pages/player_identity.rs`).
- No UI to confirm a toon that was never detected by a folder (manual names already cover "accounts that cannot be detected" via free text).

## Verification
- Native: `operator_toons` unions live + persisted, dedups a toon present in both, a removed-folder toon stays if confirmed.
- Server integration test through `crate::api_tests`: confirm → appears in a subsequent `operator_toons`-filtered read even after that folder is dropped from `PUT /api/standalone/folders`; unconfirm removes it; confirming an unknown/empty toon is rejected.
- Browser: Playwright — detected row shows the button, click confirms (persists across reload), unconfirm button removes it, admin-gating (401 without the admin token) is respected like the existing settings endpoint.
- `cargo test --workspace`, native + wasm clippy `-D warnings`, `trunk build --release` for `web-rs/app`, bundle under 1,250,000 B Brotli.

**Status: APPROVED 2026-11-21** (operator: "build all", following the chat discussion of this exact design).
