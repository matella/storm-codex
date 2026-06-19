-- État du simulateur de draft. Singleton (une seule ligne id=1) : tout l'état (config, picks/bans,
-- historique de série fearless) vit dans le JSON. Pas de table série séparée — simplicité V1.
CREATE TABLE draft_live (
    id          INT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    state       JSONB NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
