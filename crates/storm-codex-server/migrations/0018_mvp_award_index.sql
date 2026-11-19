-- Replay search (docs/specs/2026-11-19-replay-search.md): the archive's MVP filter read the first award
-- of the operator row from `match_players.data`, detoasting ~7 kB of JSON per game (~150 ms for 3,500
-- games). This partial index holds only the MVP rows, so the filter is an index-only lookup.
CREATE INDEX IF NOT EXISTS match_players_mvp_idx ON match_players (match_id, toon_handle)
    WHERE (data #>> '{gameStats,awards,0}') = 'EndOfMatchAwardMVPBoolean';
