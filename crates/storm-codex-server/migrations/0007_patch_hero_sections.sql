-- Projection des sections « héros » des patch notes → liens bidirectionnels héros ↔ patch.
-- Peuplée à l'ingestion d'un patch (live HPN ou snapshot) à partir de `sections[sectionType=Hero]`.
-- Une ligne = un héros ajusté dans un patch donné.
CREATE TABLE IF NOT EXISTS patch_hero_sections (
    patch_internal_id TEXT NOT NULL,
    anchor            TEXT NOT NULL,          -- ancre de la section dans la page patch (scroll)
    hero_short_name   TEXT,                   -- shortName HotsPatchNotes (ex. "arthas")
    hero_name         TEXT NOT NULL,          -- nom affiché (ex. "Arthas", "E.T.C.")
    hero_key          TEXT NOT NULL,          -- nom normalisé (minuscule, alphanum) pour la jointure
    classification    TEXT,                   -- BUFF | NERF | MIXED | ''
    short_summary     TEXT,
    content           TEXT,                   -- HTML de la section
    patch_name        TEXT,
    patch_type        TEXT,
    live_date         TIMESTAMPTZ,
    PRIMARY KEY (patch_internal_id, anchor)
);
CREATE INDEX IF NOT EXISTS idx_phs_hero_key ON patch_hero_sections (hero_key, live_date DESC NULLS LAST);
