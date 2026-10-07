ALTER TABLE chats ADD COLUMN last_message_preview TEXT;
ALTER TABLE chats ADD COLUMN last_message_sender_id TEXT;
ALTER TABLE chats ADD COLUMN last_message_sender_name TEXT;
ALTER TABLE chats ADD COLUMN last_message_deleted INTEGER NOT NULL DEFAULT 0;

CREATE TABLE avatars (
    user_id      TEXT PRIMARY KEY,
    bytes        BLOB,
    content_type TEXT NOT NULL DEFAULT '',
    fetched_at   INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE folders (
    id       TEXT PRIMARY KEY,
    position INTEGER NOT NULL,
    name     TEXT NOT NULL,
    kind     TEXT NOT NULL
) WITHOUT ROWID;

CREATE TABLE folder_items (
    folder_id       TEXT NOT NULL REFERENCES folders (id) ON DELETE CASCADE,
    position        INTEGER NOT NULL,
    conversation_id TEXT NOT NULL,
    PRIMARY KEY (folder_id, position)
) WITHOUT ROWID;

CREATE TABLE pinned_channels (
    position   INTEGER PRIMARY KEY,
    channel_id TEXT NOT NULL
);
