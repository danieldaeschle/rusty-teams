use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rusqlite::{Row, params};

use crate::error::Result;
use crate::models::{ChatPreview, ChatRecord, MemberRecord};
use crate::store::{Store, json_ids};
use crate::time::{optional_from_millis, optional_to_millis, to_millis};

const CHAT_COLUMNS: &str = "id, kind, title, member_summary, last_message_at, last_read_at, unread, \
     last_message_preview, last_message_sender_id, last_message_sender_name, last_message_deleted";

impl Store {
    pub fn upsert_chats(&self, chats: &[ChatRecord]) -> Result<()> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        {
            let mut upsert = transaction.prepare_cached(
                "INSERT INTO chats (id, kind, title, member_summary, last_message_at, last_read_at, unread,
                    last_message_preview, last_message_sender_id, last_message_sender_name, last_message_deleted)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT (id) DO UPDATE SET
                    kind = excluded.kind,
                    title = CASE WHEN excluded.title = '(only you)' AND chats.title NOT IN ('', '(only you)')
                        THEN chats.title ELSE excluded.title END,
                    member_summary = excluded.member_summary,
                    last_message_at = excluded.last_message_at,
                    last_read_at = COALESCE(MAX(chats.last_read_at, excluded.last_read_at), chats.last_read_at, excluded.last_read_at),
                    unread = CASE WHEN ?12 THEN chats.unread ELSE excluded.unread AND (
                        excluded.last_message_at IS NULL
                        OR COALESCE(MAX(chats.last_read_at, excluded.last_read_at), chats.last_read_at, excluded.last_read_at) IS NULL
                        OR excluded.last_message_at > COALESCE(MAX(chats.last_read_at, excluded.last_read_at), chats.last_read_at, excluded.last_read_at)
                    ) END, last_message_preview = excluded.last_message_preview,
                    last_message_sender_id = excluded.last_message_sender_id,
                    last_message_sender_name = excluded.last_message_sender_name,
                    last_message_deleted = excluded.last_message_deleted",
            )?;
            let mut clear_members =
                transaction.prepare_cached("DELETE FROM chat_members WHERE chat_id = ?1")?;
            let mut insert_member = transaction.prepare_cached(
                "INSERT INTO chat_members (chat_id, position, user_id, display_name) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for chat in chats {
                upsert.execute(params![
                    chat.id,
                    chat.kind,
                    chat.title,
                    chat.member_summary,
                    optional_to_millis(chat.last_message_at),
                    optional_to_millis(chat.last_read_at),
                    chat.unread,
                    chat.last_message_preview,
                    chat.last_message_sender_id,
                    chat.last_message_sender_name,
                    chat.last_message_deleted,
                    chat.last_event_system,
                ])?;
                clear_members.execute([&chat.id])?;
                for (position, member) in chat.members.iter().enumerate() {
                    insert_member.execute(params![
                        chat.id,
                        position as i64,
                        member.user_id,
                        member.display_name
                    ])?;
                }
            }
        }
        Ok(transaction.commit()?)
    }

    pub fn chat_last_message_times(
        &self,
        chat_ids: &[String],
    ) -> Result<HashMap<String, Option<DateTime<Utc>>>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare_cached(
            "SELECT id, last_message_at FROM chats WHERE id IN (SELECT value FROM json_each(?1))",
        )?;
        let rows = statement.query_map([json_ids(chat_ids)], |row| {
            Ok((row.get::<_, String>(0)?, optional_from_millis(row.get(1)?)))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn chat_count(&self) -> Result<usize> {
        let connection = self.lock()?;
        let count: i64 =
            connection.query_row("SELECT COUNT(*) FROM chats", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    pub fn remove_chats(&self, chat_ids: &[String]) -> Result<usize> {
        let connection = self.lock()?;
        Ok(connection.execute(
            "DELETE FROM chats WHERE id IN (SELECT value FROM json_each(?1))",
            [json_ids(chat_ids)],
        )?)
    }

    pub fn remove_chats_except(&self, kept_chat_ids: &[String]) -> Result<usize> {
        let connection = self.lock()?;
        Ok(connection.execute(
            "DELETE FROM chats WHERE id NOT IN (SELECT value FROM json_each(?1))",
            [json_ids(kept_chat_ids)],
        )?)
    }

    pub fn mark_chat_read(&self, chat_id: &str, read_at: DateTime<Utc>) -> Result<()> {
        let connection = self.lock()?;
        connection.execute(
            "UPDATE chats SET unread = 0, last_read_at = ?2 WHERE id = ?1",
            params![chat_id, to_millis(read_at)],
        )?;
        Ok(())
    }

    /// Applies the preview only when the message is at least as new as the cached last message and differs. Returns whether the row changed.
    pub fn update_chat_preview(
        &self,
        chat_id: &str,
        message_at: DateTime<Utc>,
        preview: &ChatPreview<'_>,
    ) -> Result<bool> {
        let connection = self.lock()?;
        let changed = connection.execute(
            "UPDATE chats SET last_message_at = ?2, last_message_preview = ?3, last_message_sender_id = ?4,
                last_message_sender_name = ?5, last_message_deleted = ?6
             WHERE id = ?1 AND (last_message_at IS NULL OR last_message_at <= ?2)
               AND (last_message_at IS NOT ?2 OR last_message_preview IS NOT ?3 OR last_message_sender_id IS NOT ?4
                    OR last_message_sender_name IS NOT ?5 OR last_message_deleted IS NOT ?6)",
            params![
                chat_id,
                to_millis(message_at),
                preview.text,
                preview.sender_id,
                preview.sender_name,
                preview.deleted
            ],
        )?;
        Ok(changed > 0)
    }

    pub fn recent_chats(&self, limit: usize) -> Result<Vec<ChatRecord>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare_cached(&format!(
            "SELECT {CHAT_COLUMNS} FROM chats ORDER BY last_message_at IS NULL, last_message_at DESC, id LIMIT ?1"
        ))?;
        let mut chats = statement
            .query_map([limit as i64], chat_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        load_members(&connection, &mut chats)?;
        Ok(chats)
    }

    pub fn chat(&self, chat_id: &str) -> Result<Option<ChatRecord>> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare_cached(&format!("SELECT {CHAT_COLUMNS} FROM chats WHERE id = ?1"))?;
        let mut chats = statement
            .query_map([chat_id], chat_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        load_members(&connection, &mut chats)?;
        Ok(chats.pop())
    }
}

fn chat_from_row(row: &Row<'_>) -> rusqlite::Result<ChatRecord> {
    Ok(ChatRecord {
        id: row.get(0)?,
        kind: row.get(1)?,
        title: row.get(2)?,
        member_summary: row.get(3)?,
        last_message_at: optional_from_millis(row.get(4)?),
        last_read_at: optional_from_millis(row.get(5)?),
        unread: row.get(6)?,
        members: Vec::new(),
        last_message_preview: row.get(7)?,
        last_message_sender_id: row.get(8)?,
        last_message_sender_name: row.get(9)?,
        last_message_deleted: row.get(10)?,
        last_event_system: false,
    })
}

fn load_members(connection: &rusqlite::Connection, chats: &mut [ChatRecord]) -> Result<()> {
    let mut statement = connection.prepare_cached(
        "SELECT user_id, display_name FROM chat_members WHERE chat_id = ?1 ORDER BY position",
    )?;
    for chat in chats {
        chat.members = statement
            .query_map([&chat.id], |row| {
                Ok(MemberRecord {
                    user_id: row.get(0)?,
                    display_name: row.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
    }
    Ok(())
}
