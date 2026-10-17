use rusqlite::params;

use crate::error::Result;
use crate::models::ReadHorizonRecord;
use crate::store::Store;
use crate::time::{from_millis, optional_from_millis, optional_to_millis, to_millis};

impl Store {
    pub fn read_horizons(&self, conversation_id: &str) -> Result<Vec<ReadHorizonRecord>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare_cached(
            "SELECT member_id, read_until, read_at FROM read_horizons
             WHERE conversation_id = ?1 ORDER BY member_id",
        )?;
        let rows = statement.query_map([conversation_id], |row| {
            Ok(ReadHorizonRecord {
                member_id: row.get(0)?,
                read_until: from_millis(row.get(1)?),
                read_at: optional_from_millis(row.get(2)?),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Replaces every stored horizon of the conversation.
    pub fn set_read_horizons(
        &self,
        conversation_id: &str,
        horizons: &[ReadHorizonRecord],
    ) -> Result<()> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM read_horizons WHERE conversation_id = ?1",
            [conversation_id],
        )?;
        {
            let mut statement = transaction.prepare_cached(
                "INSERT INTO read_horizons (conversation_id, member_id, read_until, read_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (conversation_id, member_id) DO UPDATE SET
                    read_until = excluded.read_until, read_at = excluded.read_at",
            )?;
            for horizon in horizons {
                statement.execute(params![
                    conversation_id,
                    horizon.member_id,
                    to_millis(horizon.read_until),
                    optional_to_millis(horizon.read_at),
                ])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }
}
