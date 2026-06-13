-- dim_talents : clé de jointure avec les choix stockés par le parser.
-- Le parser écrit player.talents[TierNChoice] = <talentTreeId> ; HotsPatchNotes expose ce même
-- identifiant via /api/heroes/{shortName}.talents[].talentTreeId. On l'indexe pour le lookup
-- (talentTreeId → nom/tier/icône) côté lecture. La table reste alimentée par refresh complet.
ALTER TABLE dim_talents ADD COLUMN IF NOT EXISTS tree_id TEXT;
CREATE INDEX IF NOT EXISTS dim_talents_tree_id_idx ON dim_talents (tree_id);
