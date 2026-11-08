-- Profil de raccourcis clavier du joueur (SUserOptionsEvent.m_hotkeyProfile), lu dans le replay.
-- Nom choisi à la main : même nom (non générique) sur deux comptes = indice fort « même personne ».
-- Spec : docs/specs/2026-11-08-profils-hotkeys-comptes-design.md. NULL = replay antérieur au re-process.
ALTER TABLE match_players ADD COLUMN IF NOT EXISTS hotkey_profile TEXT;
CREATE INDEX IF NOT EXISTS match_players_hotkey_idx
    ON match_players (lower(btrim(hotkey_profile))) WHERE hotkey_profile IS NOT NULL;
