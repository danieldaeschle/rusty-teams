use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, params};

use crate::error::Result;
use crate::store::Store;
use crate::time::{from_millis, to_millis};

impl Store {
    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        let connection = self.lock()?;
        Ok(connection
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        let connection = self.lock()?;
        connection.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn meta_time(&self, key: &str) -> Result<Option<DateTime<Utc>>> {
        Ok(self
            .meta(key)?
            .and_then(|value| value.parse().ok())
            .map(from_millis))
    }

    pub fn set_meta_time(&self, key: &str, time: DateTime<Utc>) -> Result<()> {
        self.set_meta(key, &to_millis(time).to_string())
    }
}
