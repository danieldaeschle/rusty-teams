CREATE TABLE images (
    key          TEXT PRIMARY KEY,
    bytes        BLOB NOT NULL,
    content_type TEXT NOT NULL DEFAULT '',
    fetched_at   INTEGER NOT NULL,
    size         INTEGER NOT NULL
) WITHOUT ROWID;

CREATE INDEX images_by_age ON images (fetched_at);

CREATE TABLE search_keys (
    id              INTEGER PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    message_id      TEXT NOT NULL,
    UNIQUE (conversation_id, message_id)
);

CREATE VIRTUAL TABLE message_search USING fts5(
    body, sender,
    tokenize = 'unicode61 remove_diacritics 2',
    prefix = '2 3'
);

CREATE VIRTUAL TABLE title_search USING fts5(
    title, conversation_id UNINDEXED,
    tokenize = 'unicode61 remove_diacritics 2'
);

INSERT INTO title_search (title, conversation_id) SELECT title, id FROM chats;
INSERT INTO title_search (title, conversation_id) SELECT name, id FROM channels;

CREATE TRIGGER chats_title_insert AFTER INSERT ON chats BEGIN
    INSERT INTO title_search (title, conversation_id) VALUES (new.title, new.id);
END;

CREATE TRIGGER chats_title_update AFTER UPDATE OF title ON chats WHEN old.title != new.title BEGIN
    DELETE FROM title_search WHERE conversation_id = old.id;
    INSERT INTO title_search (title, conversation_id) VALUES (new.title, new.id);
END;

CREATE TRIGGER chats_title_delete AFTER DELETE ON chats BEGIN
    DELETE FROM title_search WHERE conversation_id = old.id;
END;

CREATE TRIGGER channels_title_insert AFTER INSERT ON channels BEGIN
    INSERT INTO title_search (title, conversation_id) VALUES (new.name, new.id);
END;

CREATE TRIGGER channels_title_update AFTER UPDATE OF name ON channels WHEN old.name != new.name BEGIN
    DELETE FROM title_search WHERE conversation_id = old.id;
    INSERT INTO title_search (title, conversation_id) VALUES (new.name, new.id);
END;

CREATE TRIGGER channels_title_delete AFTER DELETE ON channels BEGIN
    DELETE FROM title_search WHERE conversation_id = old.id;
END;
