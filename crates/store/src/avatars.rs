use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, params};

use crate::error::Result;
use crate::models::AvatarRecord;
use crate::store::{Store, json_ids};
use crate::time::{from_millis, to_millis};

impl Store {
    pub fn avatar(&self, user_id: &str) -> Result<Option<AvatarRecord>> {
        let connection = self.lock()?;
        Ok(connection
            .query_row(
                "SELECT user_id, bytes, content_type, fetched_at FROM avatars WHERE user_id = ?1",
                [user_id],
                |row| {
                    Ok(AvatarRecord {
                        user_id: row.get(0)?,
                        bytes: row.get(1)?,
                        content_type: row.get(2)?,
                        fetched_at: from_millis(row.get(3)?),
                    })
                },
            )
            .optional()?)
    }

    pub fn upsert_avatar(&self, avatar: &AvatarRecord) -> Result<()> {
        let connection = self.lock()?;
        connection.execute(
            "INSERT INTO avatars (user_id, bytes, content_type, fetched_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (user_id) DO UPDATE SET
                bytes = excluded.bytes, content_type = excluded.content_type, fetched_at = excluded.fetched_at",
            params![
                avatar.user_id,
                avatar.bytes,
                avatar.content_type,
                to_millis(avatar.fetched_at)
            ],
        )?;
        Ok(())
    }

    /// Ids with no avatar row or a row fetched before `cutoff`.
    pub fn avatar_ids_needing_fetch(
        &self,
        user_ids: &[String],
        cutoff: DateTime<Utc>,
    ) -> Result<Vec<String>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare_cached(
            "SELECT value FROM json_each(?1)
             WHERE value NOT IN (SELECT user_id FROM avatars WHERE fetched_at >= ?2)",
        )?;
        let rows = statement.query_map(params![json_ids(user_ids), to_millis(cutoff)], |row| {
            row.get::<_, String>(0)
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}
