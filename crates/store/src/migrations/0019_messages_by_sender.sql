CREATE INDEX IF NOT EXISTS messages_by_sender_time ON messages (conversation_id, sender_id, created_at DESC, message_id DESC);
