-- Ligues : regroupement au-dessus des équipes (parité SotS). Léger — une colonne texte sur
-- `teams` ; la page Leagues groupe les équipes par ligue. Pas de table dédiée (usage solo,
-- définitions manuelles).
ALTER TABLE teams ADD COLUMN IF NOT EXISTS league text;
