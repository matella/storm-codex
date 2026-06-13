-- Réglages applicatifs clé/valeur (JSONB). Premier usage : `operator_names` — la liste des noms
-- en jeu de l'opérateur (plusieurs comptes) pour afficher SA perspective partout (widget, session,
-- liste de matchs) et cibler le brief Jarvis. Éditable dans l'UI Admin.
CREATE TABLE IF NOT EXISTS app_settings (
    key   text PRIMARY KEY,
    value jsonb NOT NULL
);

-- seed : auto-détecté = le joueur présent dans (quasi) toutes les parties de l'archive.
INSERT INTO app_settings (key, value)
VALUES ('operator_names', '["matella"]'::jsonb)
ON CONFLICT (key) DO NOTHING;
