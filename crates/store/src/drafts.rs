use std::collections::HashMap;

use rusqlite::{OptionalExtension, params};

use crate::attachment_images::{DRAFT_OWNER, delete_images, load_images, replace_images};
use crate::error::Result;
use crate::models::DraftRecord;
use crate::store::Store;
use crate::time::{from_millis, to_millis};

impl Store {
    pub fn save_draft(&self, record: &DraftRecord) -> Result<()> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO drafts (conversation_id, payload, preview, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (conversation_id) DO UPDATE SET
                payload = excluded.payload, preview = excluded.preview,
                updated_at = excluded.updated_at",
            params![
                record.conversation_id,
                record.payload,
                record.preview,
                to_millis(record.updated_at),
            ],
        )?;
        replace_images(
            &transaction,
            DRAFT_OWNER,
            &record.conversation_id,
            &record.images,
        )?;
        Ok(transaction.commit()?)
    }

    pub fn save_draft_text(&self, record: &DraftRecord) -> Result<()> {
        self.lock()?.execute(
            "INSERT INTO drafts (conversation_id, payload, preview, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (conversation_id) DO UPDATE SET
                payload = excluded.payload, preview = excluded.preview,
                updated_at = excluded.updated_at",
            params![
                record.conversation_id,
                record.payload,
                record.preview,
                to_millis(record.updated_at),
            ],
        )?;
        Ok(())
    }

    pub fn draft(&self, conversation_id: &str) -> Result<Option<DraftRecord>> {
        let connection = self.lock()?;
        let found = connection
            .query_row(
                "SELECT payload, preview, updated_at FROM drafts WHERE conversation_id = ?1",
                [conversation_id],
                |row| {
                    Ok(DraftRecord {
                        conversation_id: conversation_id.to_owned(),
                        payload: row.get(0)?,
                        preview: row.get(1)?,
                        images: Vec::new(),
                        updated_at: from_millis(row.get(2)?),
                    })
                },
            )
            .optional()?;
        found
            .map(|mut record| {
                record.images = load_images(&connection, DRAFT_OWNER, conversation_id)?;
                Ok(record)
            })
            .transpose()
    }

    pub fn delete_draft(&self, conversation_id: &str) -> Result<()> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM drafts WHERE conversation_id = ?1",
            [conversation_id],
        )?;
        delete_images(&transaction, DRAFT_OWNER, conversation_id)?;
        Ok(transaction.commit()?)
    }

    pub fn draft_previews(&self) -> Result<HashMap<String, String>> {
        let connection = self.lock()?;
        let mut statement =
            connection.prepare_cached("SELECT conversation_id, preview FROM drafts")?;
        let previews = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(previews)
    }
}
