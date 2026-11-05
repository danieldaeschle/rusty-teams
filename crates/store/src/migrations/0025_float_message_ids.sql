DELETE FROM message_search WHERE rowid IN (SELECT id FROM search_keys WHERE message_id GLOB '*[0-9].0');
DELETE FROM search_keys WHERE message_id GLOB '*[0-9].0';
DELETE FROM messages WHERE message_id GLOB '*[0-9].0';
