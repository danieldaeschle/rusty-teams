use rusqlite::{Connection, params};

use crate::error::Result;
use crate::models::AttachmentImage;

pub(crate) const OUTBOX_OWNER: &str = "outbox";
pub(crate) const DRAFT_OWNER: &str = "draft";

pub(crate) fn replace_images(
    connection: &Connection,
    owner_kind: &str,
    owner_id: &str,
    images: &[AttachmentImage],
) -> Result<()> {
    delete_images(connection, owner_kind, owner_id)?;
    let mut insert = connection.prepare_cached(
        "INSERT INTO attachment_images (owner_kind, owner_id, position, name, format, bytes, width, height)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    for (position, image) in images.iter().enumerate() {
        insert.execute(params![
            owner_kind,
            owner_id,
            position as i64,
            image.name,
            image.format,
            image.bytes,
            image.width,
            image.height,
        ])?;
    }
    Ok(())
}

pub(crate) fn load_images(
    connection: &Connection,
    owner_kind: &str,
    owner_id: &str,
) -> Result<Vec<AttachmentImage>> {
    let mut statement = connection.prepare_cached(
        "SELECT name, format, bytes, width, height FROM attachment_images
         WHERE owner_kind = ?1 AND owner_id = ?2 ORDER BY position",
    )?;
    let images = statement
        .query_map(params![owner_kind, owner_id], |row| {
            Ok(AttachmentImage {
                name: row.get(0)?,
                format: row.get(1)?,
                bytes: row.get(2)?,
                width: row.get(3)?,
                height: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(images)
}

pub(crate) fn delete_images(
    connection: &Connection,
    owner_kind: &str,
    owner_id: &str,
) -> Result<()> {
    connection.execute(
        "DELETE FROM attachment_images WHERE owner_kind = ?1 AND owner_id = ?2",
        params![owner_kind, owner_id],
    )?;
    Ok(())
}
