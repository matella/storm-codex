CREATE TABLE manual_sessions (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY CHECK (id > 0),
    started_at TIMESTAMPTZ NOT NULL,
    ended_at TIMESTAMPTZ CHECK (ended_at >= started_at),
    name TEXT CHECK (char_length(name) BETWEEN 1 AND 80)
);

CREATE UNIQUE INDEX manual_sessions_one_open ON manual_sessions ((true)) WHERE ended_at IS NULL;

-- No row = use the time window; NULL target in an existing row = force automatic grouping.
CREATE TABLE match_session_overrides (
    match_id BIGINT PRIMARY KEY REFERENCES matches(id) ON DELETE CASCADE,
    manual_session_id BIGINT REFERENCES manual_sessions(id)
);
