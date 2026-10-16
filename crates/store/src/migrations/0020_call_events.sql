ALTER TABLE messages ADD COLUMN call_event_json TEXT;
DELETE FROM sync_state WHERE conversation_id IN (SELECT id FROM chats);
