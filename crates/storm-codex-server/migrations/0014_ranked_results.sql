-- Résultats de rang lus à l'écran de fin de partie (Aegis, OCR). Une seule lecture par session (UNIQUE) :
-- la dernière `valide` l'emporte, une `indisponible` n'écrase jamais une `valide`. Rattachée à la partie via
-- le lobby lié (`match_id`), et retrouvée après un re-process grâce à `match_fingerprint` (l'empreinte est
-- stable, l'id du match ne l'est pas). `status` = 'valide' ou 'indisponible' : rien n'est estimé.
-- Spec : docs/specs/2026-10-31-ocr-rang-fin-de-partie-design.md.
CREATE TABLE ranked_results (
    id                  BIGSERIAL PRIMARY KEY,
    session_id          TEXT NOT NULL CHECK (session_id ~ '^[0-9a-f]{64}$'),
    status              TEXT NOT NULL CHECK (status IN ('valide', 'indisponible')),
    rank_name           TEXT,
    total_points        INT CHECK (total_points BETWEEN 0 AND 1000),
    promotion_required  INT,
    points_delta        INT CHECK (points_delta BETWEEN -1000 AND 1000),
    reason              TEXT,
    token_id            BIGINT REFERENCES upload_tokens(id) ON DELETE SET NULL,
    match_id            BIGINT REFERENCES matches(id) ON DELETE SET NULL,
    match_fingerprint   TEXT,
    read_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (status <> 'valide' OR (rank_name IS NOT NULL AND total_points IS NOT NULL)),
    CHECK (status <> 'indisponible' OR (reason IS NOT NULL AND total_points IS NULL
                                        AND promotion_required IS NULL AND points_delta IS NULL)),
    CHECK (promotion_required IS NULL OR total_points + promotion_required = 1000)
);

CREATE UNIQUE INDEX ranked_results_session_uq ON ranked_results (session_id);
CREATE INDEX ranked_results_match_idx ON ranked_results (match_id);
CREATE INDEX ranked_results_fingerprint_idx ON ranked_results (match_fingerprint);
