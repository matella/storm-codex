-- Reference dataset adapter (spec docs/specs/2026-11-15-reference-dataset-adapter-design.md, D6).
-- The patch identity becomes the HotsPatchNotes dataset id (`live:<build>`, `source:…`); the old
-- HotsPatchNotes internalIds survive as `aliases`, so `/api/patches/{id}` and `/patch/<id>` keep
-- resolving them. The patch tables are emptied: the first ingest is a "first seed" and sends no
-- `patch.new` notification. Heroes and talents are fully rewritten by that ingest.
TRUNCATE dim_patches;
TRUNCATE patch_hero_sections;
ALTER TABLE dim_patches ADD COLUMN aliases TEXT[] NOT NULL DEFAULT '{}';
CREATE INDEX dim_patches_aliases_idx ON dim_patches USING GIN (aliases);
-- `live` or `ptr`: PTR candidates are kept out of /api/hero-changes.
ALTER TABLE patch_hero_sections ADD COLUMN channel TEXT;
-- The standalone seed re-runs from the new bundle (its file hash changes anyway).
DELETE FROM app_settings WHERE key = 'reference_snapshot_sha256';
