ALTER TABLE agent_proposals ADD COLUMN mentions TEXT NOT NULL DEFAULT '[]';

CREATE TABLE agent_reaction_proposals (
    id              TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    message_id      TEXT NOT NULL,
    emoji           TEXT NOT NULL,
    agent           TEXT,
    session_id      TEXT,
    created_at      INTEGER NOT NULL,
    state           TEXT NOT NULL,
    resolved_at     INTEGER
);

CREATE INDEX agent_reaction_proposals_by_state ON agent_reaction_proposals (state, created_at);
