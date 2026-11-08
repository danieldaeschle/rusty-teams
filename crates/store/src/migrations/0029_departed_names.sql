UPDATE chat_members SET display_name = COALESCE(
    (SELECT sender_name FROM messages
     WHERE sender_id = chat_members.user_id AND sender_name NOT IN ('', '?', 'Unknown User')
     ORDER BY created_at DESC LIMIT 1),
    'Unknown User')
WHERE display_name = '?';

UPDATE chats SET member_summary = COALESCE(
    (SELECT group_concat(display_name, ', ') FROM chat_members
     WHERE chat_id = chats.id
       AND user_id IS NOT (SELECT value FROM meta WHERE key = 'me_user_id')),
    member_summary)
WHERE member_summary = '?' OR member_summary LIKE '?, %'
   OR member_summary LIKE '%, ?' OR member_summary LIKE '%, ?, %';

UPDATE chats SET title = member_summary
WHERE title = '?' OR title LIKE '?, %' OR title LIKE '%, ?' OR title LIKE '%, ?, %';
