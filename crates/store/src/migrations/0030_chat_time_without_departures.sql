CREATE TEMP TABLE chat_activity AS
SELECT conversation_id, MAX(created_at) AS active_at
FROM messages
WHERE call_event_json IS NULL
   OR NOT json_valid(call_event_json)
   OR COALESCE(json_extract(call_event_json, '$.roster.change'), 'Added') = 'Added'
GROUP BY conversation_id;

UPDATE chats
SET last_message_at = (SELECT active_at FROM chat_activity WHERE conversation_id = chats.id),
    preview_at = (SELECT active_at FROM chat_activity WHERE conversation_id = chats.id)
WHERE (SELECT active_at FROM chat_activity WHERE conversation_id = chats.id) < chats.last_message_at
  AND (
    EXISTS (
        SELECT 1 FROM messages
        WHERE conversation_id = chats.id
          AND created_at = chats.last_message_at
          AND json_valid(call_event_json)
          AND json_extract(call_event_json, '$.roster.change') <> 'Added'
    )
    OR (chats.kind = 'meeting' AND chats.last_message_preview IS NULL AND chats.last_message_sender_id IS NULL)
  );

DROP TABLE chat_activity;
