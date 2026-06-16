-- Référentiel patch notes (répliqué depuis HotsPatchNotes au démarrage, comme dim_heroes/dim_talents).
-- La liste vit ici → storm-codex en est propriétaire (détecte les nouveaux patches, sert la liste).
-- Le détail (content markdown) reste proxifié à la demande (gros volume) jusqu'au snapshot complet.
CREATE TABLE dim_patches (
    internal_id TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    type        TEXT,
    live_date   TIMESTAMPTZ,
    hero_count  INT,
    map_count   INT,
    data        JSONB NOT NULL DEFAULT '{}'   -- item brut (officialLink, etc.)
);
CREATE INDEX dim_patches_live_date_idx ON dim_patches (live_date DESC NULLS LAST);
