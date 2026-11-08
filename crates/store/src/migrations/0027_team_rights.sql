ALTER TABLE team_layout ADD COLUMN owner INTEGER NOT NULL DEFAULT 0;
ALTER TABLE team_layout ADD COLUMN add_channel INTEGER NOT NULL DEFAULT 0;
ALTER TABLE team_layout ADD COLUMN add_private_channel INTEGER NOT NULL DEFAULT 0;
ALTER TABLE team_layout ADD COLUMN edit_channel INTEGER NOT NULL DEFAULT 0;
ALTER TABLE team_layout ADD COLUMN delete_channel INTEGER NOT NULL DEFAULT 0;
ALTER TABLE channel_layout ADD COLUMN substrate_group_id TEXT;
DELETE FROM meta WHERE key = 'team_layout_refreshed_at';
