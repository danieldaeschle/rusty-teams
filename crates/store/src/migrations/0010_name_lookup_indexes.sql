CREATE INDEX messages_by_sender ON messages (sender_id, created_at);
CREATE INDEX chat_members_by_user ON chat_members (user_id);
