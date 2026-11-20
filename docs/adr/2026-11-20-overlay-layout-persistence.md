# ADR: Store OBS layouts in PostgreSQL

- Status: Accepted in `docs/specs/2026-11-20-overlay-builder.md`
- Date: 2026-11-20
- Decision owner: Storm Codex maintainer

## Context

Custom OBS layouts are user data. OBS Browser Sources need a stable URL and cannot attach the
app's admin Bearer header. Desktop backups already move the PostgreSQL data and replay archive.

## Decision

Use a UUID-addressed layout row and foreign-keyed widget rows in the existing database. Save the
layout and its placements as one snapshot; update the existing parent to keep its URL stable.
Reuse Admin auth for CRUD and expose only a public read by UUID, not a public enumeration endpoint.
The response contains the layout definition, not live widget data. The UUID is a sharing link,
not an authentication credential.

## Options considered

- Browser local storage: no database changes, but unavailable to a separate OBS browser and not
  included in the app backup.
- A separate file or store: usable by OBS but a second persistence and restore path to maintain.
- PostgreSQL (chosen): uses existing backup/restore, transactions and integrity checks; requires an
  additive migration and keeps the public read contract deliberate.

## Consequences and verification

The layout cannot be reconstructed from replays; preserve both tables in backups. Test the admin
and public route separation, replacement and FK cascade against PostgreSQL, and run the desktop
supervisor's data-only staged import with a layout and dependent widget in the dump.
