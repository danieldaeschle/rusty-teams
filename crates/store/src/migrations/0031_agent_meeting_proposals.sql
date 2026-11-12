CREATE TABLE IF NOT EXISTS agent_meeting_proposals (
    id TEXT PRIMARY KEY,
    subject TEXT NOT NULL,
    start_at INTEGER NOT NULL,
    end_at INTEGER NOT NULL,
    attendees TEXT NOT NULL DEFAULT '[]',
    description TEXT NOT NULL DEFAULT '',
    agent TEXT,
    session_id TEXT,
    created_at INTEGER NOT NULL
);
