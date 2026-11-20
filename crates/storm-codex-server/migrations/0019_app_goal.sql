-- Daily goal tracker (docs/specs/2026-11-20-streamer-widgets.md, widget 7). Single active goal, no history:
-- `id` is pinned to 1 so the application can always UPSERT instead of juggling row lifecycles; clearing the
-- goal deletes the row. Progress itself is never stored, only recomputed on each read of
-- `GET /api/goal/progress` from the existing `matches`/`match_players` archive.
CREATE TABLE IF NOT EXISTS app_goal (
    id INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    kind TEXT NOT NULL CHECK (kind IN ('wins_today', 'games_today')),
    target INTEGER NOT NULL CHECK (target > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
