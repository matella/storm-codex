-- Lien upload → match (pour retrouver le fichier archivé d'un match : dump /raw, jalon 3 T7).
ALTER TABLE uploads ADD COLUMN match_id BIGINT REFERENCES matches(id) ON DELETE SET NULL;
CREATE INDEX uploads_match_idx ON uploads(match_id);
