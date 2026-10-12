use chrono::{DateTime, Utc};
use rusqlite::{Row, params};

use crate::error::Result;
use crate::models::AgentProposalRecord;
use crate::store::Store;
use crate::time::{from_millis, optional_from_millis, optional_to_millis, to_millis};

const PENDING_STATE: &str = "pending";

const PROPOSAL_COLUMNS: &str = "id, conversation_id, reply_to_message_id, text, agent, session_id, created_at, state, resolved_at";

impl Store {
    pub fn insert_agent_proposal(&self, proposal: &AgentProposalRecord) -> Result<()> {
        let connection = self.lock()?;
        connection.execute(
            &format!(
                "INSERT INTO agent_proposals ({PROPOSAL_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"
            ),
            params![
                proposal.id,
                proposal.conversation_id,
                proposal.reply_to_message_id,
                proposal.text,
                proposal.agent,
                proposal.session_id,
                to_millis(proposal.created_at),
                proposal.state,
                optional_to_millis(proposal.resolved_at),
            ],
        )?;
        Ok(())
    }

    pub fn set_agent_proposal_state(
        &self,
        proposal_id: &str,
        state: &str,
        resolved_at: Option<DateTime<Utc>>,
    ) -> Result<bool> {
        let connection = self.lock()?;
        let changed = connection.execute(
            "UPDATE agent_proposals SET state = ?2, resolved_at = ?3 WHERE id = ?1",
            params![proposal_id, state, optional_to_millis(resolved_at)],
        )?;
        Ok(changed > 0)
    }

    /// Oldest first.
    pub fn pending_agent_proposals(&self) -> Result<Vec<AgentProposalRecord>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare_cached(&format!(
            "SELECT {PROPOSAL_COLUMNS} FROM agent_proposals WHERE state = ?1
             ORDER BY created_at ASC, id ASC"
        ))?;
        let proposals = statement
            .query_map([PENDING_STATE], proposal_from_row)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(proposals)
    }
}

fn proposal_from_row(row: &Row<'_>) -> rusqlite::Result<AgentProposalRecord> {
    Ok(AgentProposalRecord {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        reply_to_message_id: row.get(2)?,
        text: row.get(3)?,
        agent: row.get(4)?,
        session_id: row.get(5)?,
        created_at: from_millis(row.get(6)?),
        state: row.get(7)?,
        resolved_at: optional_from_millis(row.get(8)?),
    })
}
