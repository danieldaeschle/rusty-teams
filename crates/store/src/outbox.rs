use rusqlite::{Connection, Row, params, types::Type};

use crate::attachment_images::{OUTBOX_OWNER, delete_images, load_images, replace_images};
use crate::error::Result;
use crate::models::{OutboxRecord, OutboxState, OutboxTarget};
use crate::store::Store;
use crate::time::{from_millis, to_millis};

const OUTBOX_COLUMNS: &str =
    "id, conversation_id, target, thread_root_id, payload, state, last_error, created_at";

impl Store {
    pub fn put_outbox(&self, record: &OutboxRecord) -> Result<()> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            &format!(
                "INSERT INTO outbox ({OUTBOX_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT (id) DO UPDATE SET
                    conversation_id = excluded.conversation_id, target = excluded.target,
                    thread_root_id = excluded.thread_root_id, payload = excluded.payload,
                    state = excluded.state, last_error = excluded.last_error,
                    created_at = excluded.created_at"
            ),
            params![
                record.id,
                record.conversation_id,
                record.target.as_str(),
                record.thread_root_id,
                record.payload,
                record.state.as_str(),
                record.last_error,
                to_millis(record.created_at),
            ],
        )?;
        replace_images(&transaction, OUTBOX_OWNER, &record.id, &record.images)?;
        Ok(transaction.commit()?)
    }

    pub fn outbox_for_conversation(&self, conversation_id: &str) -> Result<Vec<OutboxRecord>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare_cached(&format!(
            "SELECT {OUTBOX_COLUMNS} FROM outbox WHERE conversation_id = ?1
             ORDER BY created_at ASC, id ASC"
        ))?;
        let records = statement
            .query_map([conversation_id], outbox_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        with_images(&connection, records)
    }

    pub fn sending_outbox(&self) -> Result<Vec<OutboxRecord>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare_cached(&format!(
            "SELECT {OUTBOX_COLUMNS} FROM outbox WHERE state = ?1 ORDER BY created_at ASC, id ASC"
        ))?;
        let records = statement
            .query_map([OutboxState::Sending.as_str()], outbox_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        with_images(&connection, records)
    }

    pub fn failed_outbox_conversations(&self) -> Result<Vec<String>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare_cached(
            "SELECT DISTINCT conversation_id FROM outbox WHERE state = ?1 ORDER BY conversation_id",
        )?;
        let ids = statement
            .query_map([OutboxState::Failed.as_str()], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(ids)
    }

    pub fn mark_outbox_failed(&self, id: &str, error: &str) -> Result<()> {
        let connection = self.lock()?;
        connection.execute(
            "UPDATE outbox SET state = ?2, last_error = ?3 WHERE id = ?1",
            params![id, OutboxState::Failed.as_str(), error],
        )?;
        Ok(())
    }

    pub fn delete_outbox(&self, id: &str) -> Result<()> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM outbox WHERE id = ?1", [id])?;
        delete_images(&transaction, OUTBOX_OWNER, id)?;
        Ok(transaction.commit()?)
    }
}

fn with_images(connection: &Connection, records: Vec<OutboxRecord>) -> Result<Vec<OutboxRecord>> {
    records
        .into_iter()
        .map(|mut record| {
            record.images = load_images(connection, OUTBOX_OWNER, &record.id)?;
            Ok(record)
        })
        .collect()
}

fn outbox_from_row(row: &Row<'_>) -> rusqlite::Result<OutboxRecord> {
    let target: String = row.get(2)?;
    let state: String = row.get(5)?;
    Ok(OutboxRecord {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        target: OutboxTarget::parse(&target).ok_or_else(|| unknown_value(2, "outbox target"))?,
        thread_root_id: row.get(3)?,
        payload: row.get(4)?,
        images: Vec::new(),
        state: OutboxState::parse(&state).ok_or_else(|| unknown_value(5, "outbox state"))?,
        last_error: row.get(6)?,
        created_at: from_millis(row.get(7)?),
    })
}

fn unknown_value(column: usize, what: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, Type::Text, format!("unknown {what}").into())
}
