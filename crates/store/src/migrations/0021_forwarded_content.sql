DELETE FROM sync_state WHERE conversation_id IN (
    SELECT DISTINCT conversation_id FROM messages
    WHERE attachments_json LIKE '%"forwardedMessageReference"%'
);
