use chrono::{DateTime, Utc};
use rusqlite::params;

use crate::error::Result;
use crate::store::Store;
use crate::time::to_millis;

impl Store {
    pub fn presences(&self) -> Result<Vec<(String, String)>> {
        let connection = self.lock()?;
        let mut statement =
            connection.prepare_cached("SELECT user_id, availability FROM presence")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn upsert_presences(
        &self,
        presences: &[(String, String)],
        fetched_at: DateTime<Utc>,
    ) -> Result<()> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        {
            let mut statement = transaction.prepare_cached(
                "INSERT INTO presence (user_id, availability, fetched_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT (user_id) DO UPDATE SET
                    availability = excluded.availability, fetched_at = excluded.fetched_at",
            )?;
            for (user_id, availability) in presences {
                statement.execute(params![user_id, availability, to_millis(fetched_at)])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }
}
