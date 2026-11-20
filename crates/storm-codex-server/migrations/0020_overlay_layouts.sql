-- User-authored layouts (not derived from the replay archive).
CREATE TABLE overlay_layout (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 120 AND btrim(name) <> ''),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE overlay_widget_instance (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    layout_id uuid NOT NULL REFERENCES overlay_layout(id) ON DELETE CASCADE,
    widget_type text NOT NULL CHECK (widget_type IN (
        'ticker', 'widget', 'queue', 'now-playing', 'recap', 'stream-pool',
        'patch-banner', 'record-flash', 'nemesis', 'goal'
    )),
    grid_x integer NOT NULL CHECK (grid_x BETWEEN 0 AND 79),
    grid_y integer NOT NULL CHECK (grid_y BETWEEN 0 AND 44),
    grid_w integer NOT NULL CHECK (grid_w BETWEEN 1 AND 80 - grid_x::bigint),
    grid_h integer NOT NULL CHECK (grid_h BETWEEN 1 AND 45 - grid_y::bigint),
    z_order integer NOT NULL,
    settings jsonb NOT NULL,
    -- Staged imports COPY rows directly; the allowlist must also hold outside HTTP.
    CONSTRAINT overlay_widget_settings_allowlist CHECK (
        CASE WHEN jsonb_typeof(settings) <> 'object' THEN false
        WHEN widget_type = 'widget' THEN
            settings - 'me' = '{}'::jsonb AND (
                NOT (settings ? 'me') OR (
                    jsonb_typeof(settings->'me') = 'string'
                    AND char_length(settings->>'me') <= 120
                    -- Unicode Cc, excluding NUL (which JSONB already rejects).
                    AND (settings->>'me') COLLATE "C" !~ U&'[\0001-\001F\007F-\009F]'
                )
            )
        WHEN widget_type = 'now-playing' THEN
            settings - ARRAY['mini', 'reveal'] = '{}'::jsonb
            AND (NOT (settings ? 'mini') OR jsonb_typeof(settings->'mini') = 'boolean')
            AND (NOT (settings ? 'reveal') OR jsonb_typeof(settings->'reveal') = 'boolean')
        ELSE settings = '{}'::jsonb END
    )
);

CREATE INDEX overlay_widget_instance_layout_idx ON overlay_widget_instance (layout_id, z_order, id);
