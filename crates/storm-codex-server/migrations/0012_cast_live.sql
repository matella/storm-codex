-- Régie de cast : état de la série en cours. Singleton (id=1), tout l'état (équipes, pool, parties,
-- scène, diaporama) vit dans le JSON — même modèle que `draft_live`.
CREATE TABLE cast_live (
    id          INT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    state       JSONB NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
