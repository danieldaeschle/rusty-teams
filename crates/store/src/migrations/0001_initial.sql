CREATE TABLE chats (
    id              TEXT PRIMARY KEY,
    kind            TEXT NOT NULL,
    title           TEXT NOT NULL,
    member_summary  TEXT NOT NULL DEFAULT '',
    last_message_at INTEGER,
    last_read_at    INTEGER,
    unread          INTEGER NOT NULL DEFAULT 0
) WITHOUT ROWID;

CREATE INDEX chats_by_last_message ON chats (last_message_at DESC);

CREATE TABLE chat_members (
    chat_id      TEXT NOT NULL REFERENCES chats (id) ON DELETE CASCADE,
    position     INTEGER NOT NULL,
    user_id      TEXT,
    display_name TEXT NOT NULL,
    PRIMARY KEY (chat_id, position)
) WITHOUT ROWID;

CREATE TABLE teams (
    id   TEXT PRIMARY KEY,
    name TEXT NOT NULL
) WITHOUT ROWID;

CREATE TABLE channels (
    id              TEXT PRIMARY KEY,
    team_id         TEXT NOT NULL REFERENCES teams (id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    membership_type TEXT,
    last_message_at INTEGER,
    unread          INTEGER NOT NULL DEFAULT 0
) WITHOUT ROWID;

CREATE INDEX channels_by_team ON channels (team_id, name);

CREATE TABLE messages (
    conversation_id  TEXT NOT NULL,
    message_id       TEXT NOT NULL,
    reply_to_id      TEXT,
    sender_id        TEXT,
    sender_name      TEXT,
    created_at       INTEGER NOT NULL,
    edited_at        INTEGER,
    deleted          INTEGER NOT NULL DEFAULT 0,
    body_html        TEXT NOT NULL DEFAULT '',
    attachments_json TEXT NOT NULL DEFAULT '[]',
    reactions_json   TEXT NOT NULL DEFAULT '[]',
    mentions_json    TEXT NOT NULL DEFAULT '[]',
    PRIMARY KEY (conversation_id, message_id)
) WITHOUT ROWID;

CREATE INDEX messages_by_conversation_time ON messages (conversation_id, created_at DESC, message_id DESC);

CREATE TABLE sync_state (
    conversation_id TEXT PRIMARY KEY,
    newest_seen     INTEGER,
    oldest_loaded   INTEGER,
    has_more        INTEGER NOT NULL DEFAULT 1,
    older_cursor    TEXT
) WITHOUT ROWID;
