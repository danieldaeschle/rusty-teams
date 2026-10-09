use rusqlite::params;

use crate::error::Result;
use crate::models::FolderRecord;
use crate::store::Store;

impl Store {
    pub fn replace_folders(
        &self,
        folders: &[FolderRecord],
        pinned_channel_ids: &[String],
    ) -> Result<()> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM folders", [])?;
        transaction.execute("DELETE FROM pinned_channels", [])?;
        {
            let mut insert_folder = transaction.prepare_cached(
                "INSERT INTO folders (id, position, name, kind, expanded) VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            let mut insert_item = transaction.prepare_cached(
                "INSERT INTO folder_items (folder_id, position, conversation_id) VALUES (?1, ?2, ?3)",
            )?;
            for (position, folder) in folders.iter().enumerate() {
                insert_folder.execute(params![
                    folder.id,
                    position as i64,
                    folder.name,
                    folder.kind,
                    folder.expanded
                ])?;
                for (item_position, conversation_id) in folder.conversation_ids.iter().enumerate() {
                    insert_item.execute(params![
                        folder.id,
                        item_position as i64,
                        conversation_id
                    ])?;
                }
            }
            let mut insert_channel = transaction.prepare_cached(
                "INSERT INTO pinned_channels (position, channel_id) VALUES (?1, ?2)",
            )?;
            for (position, channel_id) in pinned_channel_ids.iter().enumerate() {
                insert_channel.execute(params![position as i64, channel_id])?;
            }
        }
        Ok(transaction.commit()?)
    }

    pub fn folders(&self) -> Result<Vec<FolderRecord>> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare_cached("SELECT id, name, kind, expanded FROM folders ORDER BY position")?;
        let mut folders = statement
            .query_map([], |row| {
                Ok(FolderRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    kind: row.get(2)?,
                    expanded: row.get(3)?,
                    conversation_ids: Vec::new(),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut items = connection.prepare_cached(
            "SELECT conversation_id FROM folder_items WHERE folder_id = ?1 ORDER BY position",
        )?;
        for folder in &mut folders {
            folder.conversation_ids = items
                .query_map([&folder.id], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<_>>()?;
        }
        Ok(folders)
    }

    pub fn set_folder_expanded(&self, folder_id: &str, expanded: bool) -> Result<()> {
        let connection = self.lock()?;
        connection.execute(
            "UPDATE folders SET expanded = ?2 WHERE id = ?1",
            params![folder_id, expanded],
        )?;
        Ok(())
    }

    pub fn pinned_channel_ids(&self) -> Result<Vec<String>> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare_cached("SELECT channel_id FROM pinned_channels ORDER BY position")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}
