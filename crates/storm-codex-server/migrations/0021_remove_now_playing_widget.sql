-- Removes the 'now-playing' (Spotify/Orpheus music) OBS widget: the feature was pulled before
-- release, no production layout should reference it, but drop any stray row defensively.
DELETE FROM overlay_widget_instance WHERE widget_type = 'now-playing';

-- The widget_type allowlist CHECK was added inline (0020); Postgres named it after its default
-- convention (confirmed against a real run), so it can be dropped and recreated by name.
ALTER TABLE overlay_widget_instance DROP CONSTRAINT IF EXISTS overlay_widget_instance_widget_type_check;
ALTER TABLE overlay_widget_instance ADD CONSTRAINT overlay_widget_instance_widget_type_check CHECK (widget_type IN (
    'ticker', 'widget', 'queue', 'recap', 'stream-pool',
    'patch-banner', 'record-flash', 'nemesis', 'goal'
));

ALTER TABLE overlay_widget_instance DROP CONSTRAINT overlay_widget_settings_allowlist;
ALTER TABLE overlay_widget_instance ADD CONSTRAINT overlay_widget_settings_allowlist CHECK (
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
    ELSE settings = '{}'::jsonb END
);
