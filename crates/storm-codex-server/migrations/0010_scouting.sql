-- Rapports de scouting (spec docs/specs/2026-09-26-rapports-scouting-design.md).
-- Tables DÉDIÉES : les replays adverses ne sont jamais projetés dans matches/match_players, qui
-- alimentent toutes les stats personnelles. L'isolation tient par construction.

CREATE TABLE scouting_reports (
    id                     BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    title                  TEXT        NOT NULL,
    target_name            TEXT,                          -- nom libre de l'équipe scoutée
    roster                 JSONB       NOT NULL DEFAULT '[]', -- toon_handles fixés par l'opérateur ([] = auto)
    anchors                JSONB       NOT NULL DEFAULT '[]', -- toon_handles ancres (exceptions)
    facts                  JSONB,                         -- instantané des faits, recalculé à chaque mutation
    facts_version          INT         NOT NULL DEFAULT 0,
    analysis               JSONB,                         -- analyse importée (une seule, écrasable)
    analysis_facts_version INT,
    analysis_model         TEXT,
    analysis_imported_at   TIMESTAMPTZ,
    created_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at             TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE scouting_games (
    id             BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    report_id      BIGINT      NOT NULL REFERENCES scouting_reports(id) ON DELETE CASCADE,
    fingerprint    TEXT        NOT NULL,                  -- game_fingerprint (upload.rs)
    file_sha256    TEXT        NOT NULL,
    archived_path  TEXT        NOT NULL,
    filename       TEXT,
    played_at      TIMESTAMPTZ,
    map            TEXT,
    build          INT,
    target_team    INT,                                   -- 0/1 ; NULL = équipe cible non déterminée
    target_source  TEXT,                                  -- 'roster' | 'anchor' | 'manual'
    summary        JSONB       NOT NULL,                  -- résumé compact : seule entrée du calcul des faits
    data           JSONB       NOT NULL,                  -- sortie storm-stats complète {match, players}
    parser_version INT         NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (report_id, fingerprint)
);
CREATE INDEX scouting_games_report_idx ON scouting_games (report_id);
