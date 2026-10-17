DELETE FROM sync_state WHERE conversation_id IN (SELECT id FROM chats UNION SELECT id FROM channels);
