use chrono::{DateTime, Utc};
use rusqlite::{Row, params};
use serde_json::json;

use crate::attachment_images::{PROPOSAL_OWNER, delete_images, load_images, replace_images};
use crate::error::Result;
use crate::models::{AgentProposalMention, AgentProposalRecord, AgentReactionRecord};
use crate::store::Store;
use crate::time::{from_millis, optional_from_millis, optional_to_millis, to_millis};

const PENDING_STATE: &str = "pending";

const PROPOSAL_COLUMNS: &str = "id, conversation_id, reply_to_message_id, text, agent, session_id, created_at, state, resolved_at, mentions";

const REACTION_COLUMNS: &str =
    "id, conversation_id, message_id, emoji, agent, session_id, created_at, state, resolved_at";

impl Store {
    pub fn insert_agent_proposal(&self, proposal: &AgentProposalRecord) -> Result<()> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            &format!(
                "INSERT INTO agent_proposals ({PROPOSAL_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"
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
                mentions_json(&proposal.mentions),
            ],
        )?;
        replace_images(&transaction, PROPOSAL_OWNER, &proposal.id, &proposal.images)?;
        Ok(transaction.commit()?)
    }

    pub fn set_agent_proposal_state(
        &self,
        proposal_id: &str,
        state: &str,
        resolved_at: Option<DateTime<Utc>>,
    ) -> Result<bool> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE agent_proposals SET state = ?2, resolved_at = ?3 WHERE id = ?1",
            params![proposal_id, state, optional_to_millis(resolved_at)],
        )?;
        if state != PENDING_STATE {
            delete_images(&transaction, PROPOSAL_OWNER, proposal_id)?;
        }
        transaction.commit()?;
        Ok(changed > 0)
    }

    /// Oldest first.
    pub fn pending_agent_proposals(&self) -> Result<Vec<AgentProposalRecord>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare_cached(&format!(
            "SELECT {PROPOSAL_COLUMNS} FROM agent_proposals WHERE state = ?1
             ORDER BY created_at ASC, id ASC"
        ))?;
        let mut proposals = statement
            .query_map([PENDING_STATE], proposal_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for proposal in &mut proposals {
            proposal.images = load_images(&connection, PROPOSAL_OWNER, &proposal.id)?;
        }
        Ok(proposals)
    }

    pub fn sweep_agent_proposal_images(&self) -> Result<usize> {
        let connection = self.lock()?;
        let removed = connection.execute(
            "DELETE FROM attachment_images WHERE owner_kind = ?1
             AND owner_id NOT IN (SELECT id FROM agent_proposals WHERE state = ?2)",
            params![PROPOSAL_OWNER, PENDING_STATE],
        )?;
        Ok(removed)
    }

    pub fn agent_proposal_image_count(&self, proposal_id: &str) -> Result<usize> {
        let connection = self.lock()?;
        let count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM attachment_images WHERE owner_kind = ?1 AND owner_id = ?2",
            params![PROPOSAL_OWNER, proposal_id],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    pub fn insert_agent_reaction(&self, reaction: &AgentReactionRecord) -> Result<()> {
        let connection = self.lock()?;
        connection.execute(
            &format!(
                "INSERT INTO agent_reaction_proposals ({REACTION_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"
            ),
            params![
                reaction.id,
                reaction.conversation_id,
                reaction.message_id,
                reaction.emoji,
                reaction.agent,
                reaction.session_id,
                to_millis(reaction.created_at),
                reaction.state,
                optional_to_millis(reaction.resolved_at),
            ],
        )?;
        Ok(())
    }

    pub fn set_agent_reaction_state(
        &self,
        reaction_id: &str,
        state: &str,
        resolved_at: Option<DateTime<Utc>>,
    ) -> Result<bool> {
        let connection = self.lock()?;
        let changed = connection.execute(
            "UPDATE agent_reaction_proposals SET state = ?2, resolved_at = ?3 WHERE id = ?1",
            params![reaction_id, state, optional_to_millis(resolved_at)],
        )?;
        Ok(changed > 0)
    }

    /// Oldest first.
    pub fn pending_agent_reactions(&self) -> Result<Vec<AgentReactionRecord>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare_cached(&format!(
            "SELECT {REACTION_COLUMNS} FROM agent_reaction_proposals WHERE state = ?1
             ORDER BY created_at ASC, id ASC"
        ))?;
        let reactions = statement
            .query_map([PENDING_STATE], reaction_from_row)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(reactions)
    }
}

fn mentions_json(mentions: &[AgentProposalMention]) -> String {
    json!(
        mentions
            .iter()
            .map(|mention| json!({"user_id": mention.user_id, "name": mention.name}))
            .collect::<Vec<_>>()
    )
    .to_string()
}

fn mentions_from_json(text: &str) -> Vec<AgentProposalMention> {
    let Ok(serde_json::Value::Array(entries)) = serde_json::from_str(text) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            Some(AgentProposalMention {
                user_id: entry.get("user_id")?.as_str()?.to_owned(),
                name: entry.get("name")?.as_str()?.to_owned(),
            })
        })
        .collect()
}

fn proposal_from_row(row: &Row<'_>) -> rusqlite::Result<AgentProposalRecord> {
    let mentions: String = row.get(9)?;
    Ok(AgentProposalRecord {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        reply_to_message_id: row.get(2)?,
        text: row.get(3)?,
        mentions: mentions_from_json(&mentions),
        images: Vec::new(),
        agent: row.get(4)?,
        session_id: row.get(5)?,
        created_at: from_millis(row.get(6)?),
        state: row.get(7)?,
        resolved_at: optional_from_millis(row.get(8)?),
    })
}

fn reaction_from_row(row: &Row<'_>) -> rusqlite::Result<AgentReactionRecord> {
    Ok(AgentReactionRecord {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        message_id: row.get(2)?,
        emoji: row.get(3)?,
        agent: row.get(4)?,
        session_id: row.get(5)?,
        created_at: from_millis(row.get(6)?),
        state: row.get(7)?,
        resolved_at: optional_from_millis(row.get(8)?),
    })
}
