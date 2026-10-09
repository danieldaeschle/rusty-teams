CREATE TABLE outbox (
    id              TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    target          TEXT NOT NULL,
    thread_root_id  TEXT,
    payload         TEXT NOT NULL,
    state           TEXT NOT NULL,
    last_error      TEXT,
    created_at      INTEGER NOT NULL
);

CREATE INDEX outbox_by_conversation ON outbox (conversation_id, created_at);

CREATE TABLE drafts (
    conversation_id TEXT PRIMARY KEY,
    payload         TEXT NOT NULL,
    preview         TEXT NOT NULL,
    updated_at      INTEGER NOT NULL
);

CREATE TABLE attachment_images (
    owner_kind TEXT NOT NULL,
    owner_id   TEXT NOT NULL,
    position   INTEGER NOT NULL,
    name       TEXT NOT NULL,
    format     TEXT NOT NULL,
    bytes      BLOB NOT NULL,
    width      INTEGER,
    height     INTEGER,
    PRIMARY KEY (owner_kind, owner_id, position)
) WITHOUT ROWID;
