# Standalone reference data download

Date: 2026-11-19. Approved by the operator in chat. This replaces the standalone-only bundled-snapshot requirement in the 2026-11-15 dataset adapter design. The HotsPatchNotes dataset remains the single reference source; PostgreSQL and the application updater are unchanged.

## User experience

- The default Windows installer does not include `referential.tar.gz`. Keep the bundled PostgreSQL, frontend and replay minimaps. Measure actual installer savings against the 0.1.1 draft (110,766,415 bytes); the removed archive's 66,237,721 bytes are not a measured installer delta.
- Starting the app or importing replays never waits for the first reference download. On a fresh offline install, replay browsing remains usable with replay-derived names and existing artwork fallbacks. Patch Notes explains why it is empty. Once reference data is cached, it remains usable offline. This PC shows the sync state and offers a retry.
- Existing desktop installs keep their imported data and assets during upgrade. No destructive migration, new cloud account, telemetry, or replay upload. The new network activity is a read-only retrieval of public reference data; the user can disable it.

## Independent dataset updates

- Reuse the existing `refdata` adapter and the HotsPatchNotes stable manifest; check on startup and every 24 hours while running. Use bounded retries after failure. If the installed dataset version and durable assets are complete, an unchanged manifest triggers no asset download. Dataset updates do not require a new Storm Codex build. No change to the Tauri signed application release channel or restart-to-update policy.
- Validate against the supported schema and SHA-256 values. Stage and validate the required files before activation. Preserve the last working rows and images on interrupted, corrupt, or unsupported updates; do not mark a version complete until its required assets are durable. Ensure a restore with missing assets can recover even when the DB already holds the dataset version. Make open reference-dependent pages refresh after activation, including changes with no new patch ID.
- Keep the box's refresh behavior and public API compatible. Keep the local bundle ingestion path for older desktop resources and tests; removing the bundle from the standard installer does not mean deleting cached rows or files.
- Projection currently uses `dim_talents` to correct hero attribution and falls back when it is empty. Verify whether early imports need bounded correction after the first successful sync. If needed, specify an idempotent repair before shipping; refreshing reference tables alone must not leave permanently incorrect hero stats.

## Status and controls

Standalone-only `GET /api/standalone/reference` returns `{enabled, phase, version, revision, artwork}`. `phase` is `waiting`, `syncing`, `ready`, or `error`; the `enabled` flag is separate from whether a cached dataset is usable. `version` is the active dataset version, or null; `artwork` reports, from a local check only, whether that dataset's artwork is complete on this PC (so cached rows with missing artwork are not shown as available offline); the opaque string `revision` changes when an activation becomes visible. `PUT /api/standalone/reference` with `{enabled: bool}` persists the setting, returns status, and queues a check when enabled; any preference change cancels a check in progress, which can never activate even if sync is re-enabled before it finishes; `POST /api/standalone/reference/refresh` requests one check (202; disabled: 409). Reuse existing standalone Host/Origin protection; no user-controlled feed URL through HTTP. UI checks the local status, not GitHub. Keep sync errors separate from the import phase and preserve usable data while a check is running.

## Acceptance

1. With a clean Windows profile and network unavailable, install (with the supported WebView2 prerequisite), import a real replay and browse it without waiting for reference sync.
2. Restore connectivity: activation fills reference-dependent views without an app restart. Inject a newer fixture dataset, and verify unchanged, rejected and interrupted refreshes, last-good retention, and restart/offline behavior. Check upgrade and restore from 0.1.1.
3. Compare the official NSIS build against the 0.1.1 draft; record installer bytes, first-sync transfer, and startup/import timings. No release publication or modification of the existing app updater is part of this design.
