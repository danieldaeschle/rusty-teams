CREATE TABLE agent_proposals (
    id                  TEXT PRIMARY KEY,
    conversation_id     TEXT NOT NULL,
    reply_to_message_id TEXT,
    text                TEXT NOT NULL,
    agent               TEXT,
    session_id          TEXT,
    created_at          INTEGER NOT NULL,
    state               TEXT NOT NULL,
    resolved_at         INTEGER
);

CREATE INDEX agent_proposals_by_conversation_state ON agent_proposals (conversation_id, state);
