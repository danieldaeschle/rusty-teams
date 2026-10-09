ALTER TABLE channel_layout ADD COLUMN notification_level TEXT NOT NULL DEFAULT 'feed';
ALTER TABLE channel_layout ADD COLUMN include_replies INTEGER NOT NULL DEFAULT 0;
