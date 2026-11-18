# Standalone identity and import polish

Date: 2026-11-18. Scope approved in chat: latest owner name per replay folder, live completed-replay verification, and stable import progress. The potential Windows console flash needs an installed-build reproduction; do not change PostgreSQL launch or packaging in this branch.

## Identity

- For a standard HotS replay folder, the ToonHandle in the folder path identifies its owner. Match that handle in the local replay projection and show the latest nonblank name by match play time, with a deterministic tie break. Do not infer ownership from the most frequent name, modification time, or insertion order. For extra folders without a verifiable handle, use the existing manual identity editor rather than guessing.
- List automatic detections by folder separately from editable manual names. Make verified local identities available to existing “me” views without overwriting manual entries or losing older aliases. Keep box behavior unchanged. Be explicit about any legacy seed/auto-detected name that cannot safely be distinguished from a manual choice.
- Refresh after backfill (including an index/ledger no-op or restart) and after a successful live completed-replay import. When a replay arrives out of order, the newest play time still wins. The open app must pick up the update without a reload and must not clobber a dirty manual edit. No new external service, dependency, or in-progress game producer.
- Prove with a valid watched replay from a standard folder arriving after readiness, then check the projected match and visible identity. Test two owners, older/newer aliases, and an unrelated frequent teammate; flat folder ownership stays unknown. New account folders can still require Rescan.

## Import progress

- Do not publish a new `Importing` phase with an uninitialized `0/0` counter. A no-op rescan in Ready leaves the settled state in place. Keep the active import panel mounted across ordinary updates, and ignore stale/late responses so a poll cannot revert a newer folder edit or progress snapshot. Distinguish worker queue failures from successfully queued rescans.
- Test the zero-file rescan and initialized progress with a watcher/server fixture. Test delayed response ordering and the UI status path; verify in a browser with a temporary instance, not the live service.

## Out of scope

No packaging change, recovery redesign, live in-game capture, or unrelated fixes from the completeness audit. An installed Windows acceptance run still needs MH-Home. Update the matching living specs and `docs/STATUS.md`; do not push or deploy as part of this implementation pass.
