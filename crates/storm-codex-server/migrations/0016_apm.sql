-- Actions par minute du joueur dans la partie : SCmdEvent du joueur / minutes de jeu.
-- Définition et limites : docs/specs/2026-11-08-apm-par-partie-design.md. NULL = replay antérieur
-- au re-process (PARSER_VERSION 4) ou durée de partie inconnue.
ALTER TABLE match_players ADD COLUMN IF NOT EXISTS apm DOUBLE PRECISION;
