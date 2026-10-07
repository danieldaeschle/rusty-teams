use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, params};

use crate::error::Result;
use crate::models::ImageRecord;
use crate::store::Store;
use crate::time::{from_millis, to_millis};

impl Store {
    pub fn image(&self, key: &str) -> Result<Option<ImageRecord>> {
        let connection = self.lock()?;
        Ok(connection
            .query_row(
                "SELECT key, bytes, content_type, fetched_at FROM images WHERE key = ?1",
                [key],
                |row| {
                    Ok(ImageRecord {
                        key: row.get(0)?,
                        bytes: row.get(1)?,
                        content_type: row.get(2)?,
                        fetched_at: from_millis(row.get(3)?),
                    })
                },
            )
            .optional()?)
    }

    pub fn has_image(&self, key: &str) -> Result<bool> {
        let connection = self.lock()?;
        Ok(connection
            .query_row("SELECT 1 FROM images WHERE key = ?1", [key], |_| Ok(()))
            .optional()?
            .is_some())
    }

    /// Stores the image, then evicts the oldest others until the total is within `max_total_bytes`.
    pub fn put_image(&self, image: &ImageRecord, max_total_bytes: u64) -> Result<()> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO images (key, bytes, content_type, fetched_at, size) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (key) DO UPDATE SET
                bytes = excluded.bytes, content_type = excluded.content_type,
                fetched_at = excluded.fetched_at, size = excluded.size",
            params![
                image.key,
                image.bytes,
                image.content_type,
                to_millis(image.fetched_at),
                image.bytes.len() as i64
            ],
        )?;
        let mut total: i64 =
            transaction.query_row("SELECT COALESCE(SUM(size), 0) FROM images", [], |row| {
                row.get(0)
            })?;
        if total > max_total_bytes as i64 {
            let oldest = transaction
                .prepare(
                    "SELECT key, size FROM images WHERE key != ?1 ORDER BY fetched_at ASC, key ASC",
                )?
                .query_map([&image.key], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for (key, size) in oldest {
                if total <= max_total_bytes as i64 {
                    break;
                }
                transaction.execute("DELETE FROM images WHERE key = ?1", [key])?;
                total -= size;
            }
        }
        Ok(transaction.commit()?)
    }

    pub fn image_cache_bytes(&self) -> Result<u64> {
        let connection = self.lock()?;
        let total: i64 =
            connection.query_row("SELECT COALESCE(SUM(size), 0) FROM images", [], |row| {
                row.get(0)
            })?;
        Ok(total as u64)
    }

    pub fn delete_images_before(&self, cutoff: DateTime<Utc>) -> Result<usize> {
        let connection = self.lock()?;
        Ok(connection.execute(
            "DELETE FROM images WHERE fetched_at < ?1",
            [to_millis(cutoff)],
        )?)
    }
}
