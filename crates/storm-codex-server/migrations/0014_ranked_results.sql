-- Résultats de rang lus à l'écran de fin de partie (Aegis, OCR). Une ligne par lecture, rattachée à
-- la partie par `session_id` (SHA-256 hex du battlelobby, comme le feed live). `status` = 'valide' ou
-- 'indisponible' : rien n'est estimé. Spec : docs/specs/2026-10-31-ocr-rang-fin-de-partie-design.md.
CREATE TABLE ranked_results (
    id                  BIGSERIAL PRIMARY KEY,
    session_id          TEXT NOT NULL CHECK (session_id ~ '^[0-9a-f]{64}$'),
    status              TEXT NOT NULL CHECK (status IN ('valide', 'indisponible')),
    rank_name           TEXT,
    total_points        INT,
    promotion_required  INT,
    points_delta        INT,
    reason              TEXT,
    token_id            BIGINT REFERENCES upload_tokens(id),
    match_id            BIGINT REFERENCES matches(id) ON DELETE SET NULL,
    read_at             TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX ranked_results_session_idx ON ranked_results (session_id, read_at DESC);
CREATE INDEX ranked_results_match_idx ON ranked_results (match_id);
