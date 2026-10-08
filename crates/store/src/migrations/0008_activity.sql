CREATE TABLE activity (
    id              INTEGER PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    kind            TEXT NOT NULL,
    message_id      TEXT NOT NULL,
    actors_json     TEXT NOT NULL,
    preview         TEXT NOT NULL,
    glyphs          TEXT NOT NULL,
    count           INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    read            INTEGER NOT NULL
);

CREATE INDEX activity_by_time ON activity (updated_at);
