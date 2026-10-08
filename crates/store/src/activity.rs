use chrono::{DateTime, Utc};
use rusqlite::{Row, params};

use crate::error::Result;
use crate::models::ActivityRecord;
use crate::store::Store;
use crate::time::{from_millis, to_millis};

const ACTIVITY_COLUMNS: &str =
    "id, conversation_id, kind, message_id, actors_json, preview, glyphs, count, updated_at, read";

impl Store {
    pub fn upsert_activity(&self, records: &[ActivityRecord]) -> Result<()> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        {
            let mut upsert = transaction.prepare_cached(&format!(
                "INSERT INTO activity ({ACTIVITY_COLUMNS})
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT (id) DO UPDATE SET
                    conversation_id = excluded.conversation_id, kind = excluded.kind,
                    message_id = excluded.message_id, actors_json = excluded.actors_json,
                    preview = excluded.preview, glyphs = excluded.glyphs, count = excluded.count,
                    updated_at = excluded.updated_at, read = excluded.read"
            ))?;
            for record in records {
                upsert.execute(params![
                    record.id,
                    record.conversation_id,
                    record.kind,
                    record.message_id,
                    record.actors_json,
                    record.preview,
                    record.glyphs,
                    record.count,
                    to_millis(record.updated_at),
                    record.read,
                ])?;
            }
        }
        Ok(transaction.commit()?)
    }

    /// Newest first.
    pub fn activity(&self) -> Result<Vec<ActivityRecord>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare_cached(&format!(
            "SELECT {ACTIVITY_COLUMNS} FROM activity ORDER BY updated_at DESC, id DESC"
        ))?;
        let records = statement
            .query_map([], activity_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(records)
    }

    pub fn prune_activity(&self, before: DateTime<Utc>) -> Result<usize> {
        let connection = self.lock()?;
        Ok(connection.execute(
            "DELETE FROM activity WHERE updated_at < ?1",
            [to_millis(before)],
        )?)
    }
}

fn activity_from_row(row: &Row<'_>) -> rusqlite::Result<ActivityRecord> {
    Ok(ActivityRecord {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        kind: row.get(2)?,
        message_id: row.get(3)?,
        actors_json: row.get(4)?,
        preview: row.get(5)?,
        glyphs: row.get(6)?,
        count: row.get(7)?,
        updated_at: from_millis(row.get(8)?),
        read: row.get(9)?,
    })
}
