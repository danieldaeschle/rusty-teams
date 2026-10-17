CREATE TABLE IF NOT EXISTS read_horizons (
    conversation_id TEXT NOT NULL,
    member_id TEXT NOT NULL,
    read_until INTEGER NOT NULL,
    read_at INTEGER,
    PRIMARY KEY (conversation_id, member_id)
);
