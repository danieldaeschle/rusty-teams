use rusqlite::{OptionalExtension, params};

use crate::error::Result;
use crate::models::SyncState;
use crate::store::Store;
use crate::time::{optional_from_millis, optional_to_millis};

impl Store {
    pub fn sync_state(&self, conversation_id: &str) -> Result<Option<SyncState>> {
        let connection = self.lock()?;
        let state = connection
            .query_row(
                "SELECT newest_seen, oldest_loaded, has_more, older_cursor, delta_link FROM sync_state WHERE conversation_id = ?1",
                [conversation_id],
                |row| {
                    Ok(SyncState {
                        newest_seen: optional_from_millis(row.get(0)?),
                        oldest_loaded: optional_from_millis(row.get(1)?),
                        has_more: row.get(2)?,
                        older_cursor: row.get(3)?,
                        delta_link: row.get(4)?,
                    })
                },
            )
            .optional()?;
        Ok(state)
    }

    pub fn set_sync_state(&self, conversation_id: &str, state: &SyncState) -> Result<()> {
        let connection = self.lock()?;
        connection.execute(
            "INSERT INTO sync_state (conversation_id, newest_seen, oldest_loaded, has_more, older_cursor, delta_link)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (conversation_id) DO UPDATE SET
                newest_seen = excluded.newest_seen, oldest_loaded = excluded.oldest_loaded,
                has_more = excluded.has_more, older_cursor = excluded.older_cursor,
                delta_link = excluded.delta_link",
            params![
                conversation_id,
                optional_to_millis(state.newest_seen),
                optional_to_millis(state.oldest_loaded),
                state.has_more,
                state.older_cursor,
                state.delta_link,
            ],
        )?;
        Ok(())
    }
}
