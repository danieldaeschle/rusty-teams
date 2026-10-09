CREATE TABLE channel_tabs (
    channel_id    TEXT NOT NULL,
    position      INTEGER NOT NULL,
    tab_id        TEXT NOT NULL,
    name          TEXT NOT NULL,
    definition_id TEXT NOT NULL,
    open_url      TEXT,
    PRIMARY KEY (channel_id, position)
) WITHOUT ROWID;
