-- storm-codex — schéma initial (jalon 3). Étage Postgres = projection complète des replays.
-- Stratégie : grosses structures en JSONB (projection sans perte, re-process idempotent) +
-- colonnes scalaires promues pour les axes de filtre/tri chauds (indexés). parser_version
-- partout pour le re-process. Les définitions non dérivables des replays (teams/leagues/
-- collections) se recréent à la main dans l'UI (jalon 4) — tables créées vides ici.

-- ── Tokens d'upload nominatifs ───────────────────────────────────────────────
CREATE TABLE upload_tokens (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name        TEXT NOT NULL,
    token_hash  TEXT NOT NULL UNIQUE,          -- SHA-256 hex du token (jamais le clair)
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at  TIMESTAMPTZ
);

-- ── Traçabilité des uploads (1 ligne par fichier reçu) ───────────────────────
CREATE TABLE uploads (
    id            BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    token_id      BIGINT REFERENCES upload_tokens(id),
    filename      TEXT NOT NULL,
    fingerprint   TEXT NOT NULL UNIQUE,         -- anti-doublon (MD5 BlizzIDs+random)
    archived_path TEXT,                          -- chemin du brut archivé (source de vérité)
    status        TEXT NOT NULL DEFAULT 'pending'
                  CHECK (status IN ('pending','parsed','duplicate','parse_failed')),
    error_class   TEXT,                          -- classe typée (storm_replay::Error / unsupported_map…)
    error_msg     TEXT,
    parser_version INT NOT NULL DEFAULT 0,
    build         INT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    parsed_at     TIMESTAMPTZ
);
CREATE INDEX uploads_status_idx ON uploads(status);

-- ── Matchs (projection) ──────────────────────────────────────────────────────
CREATE TABLE matches (
    id             BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    fingerprint    TEXT NOT NULL UNIQUE,
    build          INT,
    mode           INT,
    map            TEXT,
    duration_loops INT,                          -- match.loopLength
    length         DOUBLE PRECISION,             -- secondes
    played_at      TIMESTAMPTZ,                  -- match.date
    winner         INT,
    first_pick_win BOOLEAN,
    first_objective INT,
    first_fort     INT,
    first_keep     INT,
    parser_version INT NOT NULL,
    data           JSONB NOT NULL,               -- objet `match` complet (timeline, objectifs, teams…)
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX matches_map_idx ON matches(map);
CREATE INDEX matches_mode_idx ON matches(mode);
CREATE INDEX matches_played_at_idx ON matches(played_at DESC);
CREATE INDEX matches_build_idx ON matches(build);

-- ── Joueurs par match (10/match) ─────────────────────────────────────────────
CREATE TABLE match_players (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    match_id    BIGINT NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
    toon_handle TEXT NOT NULL,
    name        TEXT,
    hero        TEXT,
    team        INT,
    win         BOOLEAN,
    hero_level  INT,
    kills       INT,                             -- gameStats.SoloKill / Takedowns selon usage
    takedowns   INT,
    deaths      INT,
    hero_damage BIGINT,
    healing     BIGINT,
    experience  BIGINT,
    data        JSONB NOT NULL,                  -- objet `player` complet (gameStats, talents, takedowns…)
    UNIQUE (match_id, toon_handle)
);
CREATE INDEX match_players_toon_idx ON match_players(toon_handle);
CREATE INDEX match_players_hero_idx ON match_players(hero);
CREATE INDEX match_players_match_idx ON match_players(match_id);

-- ── Référentiel joueurs (alias/tags agrégés) ─────────────────────────────────
CREATE TABLE players (
    toon_handle TEXT PRIMARY KEY,
    last_name   TEXT,
    names       JSONB NOT NULL DEFAULT '[]',
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── Définitions manuelles (recréées dans l'UI au jalon 4) ────────────────────
CREATE TABLE teams (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name       TEXT NOT NULL,
    roster     JSONB NOT NULL DEFAULT '[]',       -- ToonHandles
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE leagues (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name       TEXT NOT NULL,
    teams      JSONB NOT NULL DEFAULT '[]',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE collections (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name       TEXT NOT NULL,
    match_ids  JSONB NOT NULL DEFAULT '[]',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── Référentiel héros/talents (répliqué depuis l'API HotsPatchNotes au démarrage, jalon 4) ──
CREATE TABLE dim_heroes (
    id        TEXT PRIMARY KEY,                   -- nom interne
    name      TEXT NOT NULL,
    role      TEXT,
    universe  TEXT,
    data      JSONB NOT NULL DEFAULT '{}'
);
CREATE TABLE dim_talents (
    hero_id   TEXT NOT NULL,
    tier      INT NOT NULL,
    name      TEXT NOT NULL,
    data      JSONB NOT NULL DEFAULT '{}',
    PRIMARY KEY (hero_id, tier, name)
);
