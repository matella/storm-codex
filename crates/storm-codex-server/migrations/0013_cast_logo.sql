-- Régie de cast : logos d'équipe (un par côté), stockés en base (≤ 512 Ko, png/jpeg/webp).
CREATE TABLE cast_logo (
    side        TEXT PRIMARY KEY CHECK (side IN ('blue', 'red')),
    mime        TEXT NOT NULL,
    bytes       BYTEA NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
