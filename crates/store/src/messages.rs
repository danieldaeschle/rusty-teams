use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, Row, params, params_from_iter};

use crate::error::Result;
use crate::models::MessageRecord;
use crate::search::index_message;
use crate::store::Store;
use crate::time::{from_millis, optional_from_millis, optional_to_millis, to_millis};

const MESSAGE_COLUMNS: &str = "conversation_id, message_id, reply_to_id, sender_id, sender_name, created_at, \
     edited_at, deleted, body_html, attachments_json, reactions_json, mentions_json, sender_application_id";

impl Store {
    pub fn upsert_messages(&self, messages: &[MessageRecord]) -> Result<()> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        {
            let mut upsert = transaction.prepare_cached(&format!(
                "INSERT INTO messages ({MESSAGE_COLUMNS})
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                 ON CONFLICT (conversation_id, message_id) DO UPDATE SET
                    reply_to_id = excluded.reply_to_id, sender_id = excluded.sender_id,
                    sender_name = excluded.sender_name, created_at = excluded.created_at,
                    edited_at = excluded.edited_at, deleted = excluded.deleted, body_html = excluded.body_html,
                    attachments_json = excluded.attachments_json, reactions_json = excluded.reactions_json,
                    mentions_json = excluded.mentions_json,
                    sender_application_id = excluded.sender_application_id"
            ))?;
            for message in messages {
                upsert.execute(params![
                    message.conversation_id,
                    message.message_id,
                    message.reply_to_id,
                    message.sender_id,
                    message.sender_name,
                    to_millis(message.created_at),
                    optional_to_millis(message.edited_at),
                    message.deleted,
                    message.body_html,
                    message.attachments_json,
                    message.reactions_json,
                    message.mentions_json,
                    message.sender_application_id,
                ])?;
                index_message(&transaction, message)?;
            }
        }
        Ok(transaction.commit()?)
    }

    pub fn count_messages_after(
        &self,
        conversation_id: &str,
        after: Option<DateTime<Utc>>,
        not_from_user_id: &str,
    ) -> Result<usize> {
        let connection = self.lock()?;
        let count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM messages
             WHERE conversation_id = ?1 AND deleted = 0 AND created_at > ?2
               AND (sender_id IS NULL OR sender_id != ?3)",
            params![
                conversation_id,
                optional_to_millis(after).unwrap_or(i64::MIN),
                not_from_user_id
            ],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    pub fn first_message_after_from_others(
        &self,
        conversation_id: &str,
        after: Option<DateTime<Utc>>,
        not_from_user_id: &str,
    ) -> Result<Option<String>> {
        let connection = self.lock()?;
        Ok(connection
            .query_row(
                "SELECT message_id FROM messages
                 WHERE conversation_id = ?1 AND deleted = 0 AND created_at > ?2
                   AND (sender_id IS NULL OR sender_id != ?3)
                 ORDER BY created_at ASC, message_id ASC LIMIT 1",
                params![
                    conversation_id,
                    optional_to_millis(after).unwrap_or(i64::MIN),
                    not_from_user_id
                ],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Oldest-first page of the `limit` newest messages strictly older than `before` (all when `None`).
    pub fn messages(
        &self,
        conversation_id: &str,
        before: Option<DateTime<Utc>>,
        limit: usize,
    ) -> Result<Vec<MessageRecord>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare_cached(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM messages
             WHERE conversation_id = ?1 AND (?2 IS NULL OR created_at < ?2)
             ORDER BY created_at DESC, message_id DESC LIMIT ?3"
        ))?;
        let mut messages = statement
            .query_map(
                params![conversation_id, optional_to_millis(before), limit as i64],
                message_from_row,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        messages.reverse();
        Ok(messages)
    }

    pub fn messages_by_id(
        &self,
        conversation_id: &str,
        message_ids: &[String],
    ) -> Result<HashMap<String, MessageRecord>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare_cached(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM messages
             WHERE conversation_id = ?1 AND message_id IN (SELECT value FROM json_each(?2))"
        ))?;
        let ids = serde_json::to_string(message_ids).unwrap_or_else(|_| "[]".to_owned());
        let rows = statement.query_map(
            params_from_iter([conversation_id, ids.as_str()]),
            message_from_row,
        )?;
        let mut found = HashMap::new();
        for row in rows {
            let message = row?;
            found.insert(message.message_id.clone(), message);
        }
        Ok(found)
    }

    pub fn display_names(&self, user_ids: &[String]) -> Result<HashMap<String, String>> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let connection = self.lock()?;
        let mut statement = connection.prepare_cached(
            "SELECT value, COALESCE(
                 (SELECT display_name FROM chat_members
                  WHERE user_id = value AND display_name <> '' LIMIT 1),
                 (SELECT sender_name FROM messages
                  WHERE sender_id = value AND sender_name <> ''
                  ORDER BY created_at DESC LIMIT 1))
             FROM json_each(?1)",
        )?;
        let ids = serde_json::to_string(user_ids).unwrap_or_else(|_| "[]".to_owned());
        let rows = statement.query_map([ids], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?;
        let mut names = HashMap::new();
        for row in rows {
            if let (user_id, Some(name)) = row? {
                names.insert(user_id, name);
            }
        }
        Ok(names)
    }

    pub fn thread_has_sender(
        &self,
        conversation_id: &str,
        root_id: &str,
        user_id: &str,
    ) -> Result<bool> {
        let connection = self.lock()?;
        Ok(connection.query_row(
            "SELECT EXISTS (
                SELECT 1 FROM messages
                WHERE conversation_id = ?1 AND sender_id = ?3 AND deleted = 0
                  AND (message_id = ?2 OR reply_to_id = ?2))",
            params![conversation_id, root_id, user_id],
            |row| row.get(0),
        )?)
    }

    pub fn message_count(&self, conversation_id: &str) -> Result<usize> {
        let connection = self.lock()?;
        let count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM messages WHERE conversation_id = ?1",
            [conversation_id],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }
}

fn message_from_row(row: &Row<'_>) -> rusqlite::Result<MessageRecord> {
    Ok(MessageRecord {
        conversation_id: row.get(0)?,
        message_id: row.get(1)?,
        reply_to_id: row.get(2)?,
        sender_id: row.get(3)?,
        sender_name: row.get(4)?,
        created_at: from_millis(row.get(5)?),
        edited_at: optional_from_millis(row.get(6)?),
        deleted: row.get(7)?,
        body_html: row.get(8)?,
        attachments_json: row.get(9)?,
        reactions_json: row.get(10)?,
        mentions_json: row.get(11)?,
        sender_application_id: row.get(12)?,
    })
}
