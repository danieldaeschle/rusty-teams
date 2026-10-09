ALTER TABLE messages ADD COLUMN subject TEXT;

UPDATE sync_state SET delta_link = NULL WHERE conversation_id IN (SELECT id FROM channels);
