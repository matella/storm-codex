-- Liens « même personne » entre comptes (spec docs/specs/2026-10-25-recherche-joueurs-design.md).
-- Une décision de l'opérateur sur une paire de toon_handle ; les suggestions elles-mêmes sont
-- calculées à la demande, jamais stockées. Aucun CREATE EXTENSION : cette migration tourne au
-- démarrage sur le Postgres partagé du box.
CREATE TABLE player_links (
    toon_a     TEXT        NOT NULL,
    toon_b     TEXT        NOT NULL,
    status     TEXT        NOT NULL,
    decided_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (toon_a, toon_b),
    CHECK (toon_a < toon_b),                       -- paire ordonnée : A↔B = une seule ligne
    CHECK (status IN ('confirmed', 'dismissed'))
);
CREATE INDEX player_links_b_idx ON player_links(toon_b);
