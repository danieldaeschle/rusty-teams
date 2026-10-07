CREATE TABLE presence (
    user_id      TEXT PRIMARY KEY,
    availability TEXT NOT NULL,
    fetched_at   INTEGER NOT NULL
) WITHOUT ROWID;
