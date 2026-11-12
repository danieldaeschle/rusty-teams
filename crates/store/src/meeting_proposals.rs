use rusqlite::{Row, params};

use crate::error::Result;
use crate::models::AgentMeetingRecord;
use crate::store::Store;
use crate::time::{from_millis, to_millis};

const MEETING_COLUMNS: &str = "id, subject, start_at, end_at, attendees, description, agent, session_id, created_at";

impl Store {
    pub fn insert_agent_meeting(&self, meeting: &AgentMeetingRecord) -> Result<()> {
        let connection = self.lock()?;
        connection.execute(
            &format!("INSERT INTO agent_meeting_proposals ({MEETING_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"),
            params![
                meeting.id,
                meeting.subject,
                to_millis(meeting.start),
                to_millis(meeting.end),
                serde_json::json!(meeting.attendees).to_string(),
                meeting.description,
                meeting.agent,
                meeting.session_id,
                to_millis(meeting.created_at),
            ],
        )?;
        Ok(())
    }

    pub fn delete_agent_meeting(&self, meeting_id: &str) -> Result<bool> {
        let connection = self.lock()?;
        let removed = connection.execute("DELETE FROM agent_meeting_proposals WHERE id = ?1", [meeting_id])?;
        Ok(removed > 0)
    }

    /// Oldest first.
    pub fn agent_meetings(&self) -> Result<Vec<AgentMeetingRecord>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare_cached(&format!(
            "SELECT {MEETING_COLUMNS} FROM agent_meeting_proposals ORDER BY created_at ASC, id ASC"
        ))?;
        let meetings = statement
            .query_map([], meeting_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(meetings)
    }
}

fn meeting_from_row(row: &Row<'_>) -> rusqlite::Result<AgentMeetingRecord> {
    let attendees: String = row.get(4)?;
    Ok(AgentMeetingRecord {
        id: row.get(0)?,
        subject: row.get(1)?,
        start: from_millis(row.get(2)?),
        end: from_millis(row.get(3)?),
        attendees: serde_json::from_str(&attendees).unwrap_or_default(),
        description: row.get(5)?,
        agent: row.get(6)?,
        session_id: row.get(7)?,
        created_at: from_millis(row.get(8)?),
    })
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use crate::models::AgentMeetingRecord;
    use crate::store::Store;

    #[test]
    fn meeting_proposals_survive_until_deleted() {
        let store = Store::open_in_memory().unwrap();
        let start = Utc.with_ymd_and_hms(2026, 11, 20, 9, 0, 0).unwrap();
        let meeting = AgentMeetingRecord {
            id: "m1".to_owned(),
            subject: "Sync".to_owned(),
            start,
            end: start + chrono::Duration::minutes(30),
            attendees: vec!["mara@example.com".to_owned()],
            description: "Agenda".to_owned(),
            agent: Some("claude".to_owned()),
            session_id: None,
            created_at: start,
        };
        store.insert_agent_meeting(&meeting).unwrap();
        assert_eq!(store.agent_meetings().unwrap(), [meeting]);
        assert!(store.delete_agent_meeting("m1").unwrap());
        assert!(store.agent_meetings().unwrap().is_empty());
    }
}
