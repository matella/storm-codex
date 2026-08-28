-- Bibliothèque de builds. `picks` a EXACTEMENT la forme écrite par le parser dans
-- match_players.data.talents : {"Tier1Choice": "<talentTreeId>", ...}. C'est ce qui rend l'import
-- depuis un match et le diff post-game de simples comparaisons d'objets, sans mapping à inventer.
CREATE TABLE builds (
    id              BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    hero_id         TEXT NOT NULL,          -- = dim_heroes.id = match_players.hero
    name            TEXT NOT NULL,
    picks           JSONB NOT NULL,
    notes           TEXT,
    is_default      BOOLEAN NOT NULL DEFAULT false,
    source_match_id BIGINT REFERENCES matches(id) ON DELETE SET NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX builds_hero_idx ON builds (hero_id);
-- Invariant tenu par la base, pas par le code : au plus un build par défaut par héros.
CREATE UNIQUE INDEX builds_one_default_per_hero ON builds (hero_id) WHERE is_default;

-- Lobby courant. Singleton, calque exact de draft_live : tout l'état dans le JSON, écrasé à chaque
-- nouveau lobby. Aucun historique — le replay archivé reste la source de vérité.
CREATE TABLE lobby_live (
    id         INT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    state      JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Résolution BattleTag → toon_handle : le blob de lobby ne porte que "nom#discriminant", et
-- l'archive est la seule table de correspondance dont on dispose (cf. spec companion-live).
CREATE INDEX match_players_name_tag_idx
    ON match_players (lower(name), (data ->> 'tag'));
