CREATE TABLE team_layout (
    team_id  TEXT PRIMARY KEY,
    position INTEGER NOT NULL,
    hidden   INTEGER NOT NULL DEFAULT 0
) WITHOUT ROWID;

CREATE TABLE channel_layout (
    channel_id TEXT PRIMARY KEY,
    general    INTEGER NOT NULL DEFAULT 0,
    hidden     INTEGER NOT NULL DEFAULT 0
) WITHOUT ROWID;
