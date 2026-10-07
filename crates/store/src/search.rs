use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::error::Result;
use crate::models::{ConversationHit, MessageRecord, SearchHit};
use crate::store::Store;
use crate::text::plain_text;
use crate::time::from_millis;

const SNIPPET_TOKENS: i64 = 12;
const SENDER_FILTER: &str = "from:";

pub const HIGHLIGHT_START: char = '\u{1}';
pub const HIGHLIGHT_END: char = '\u{2}';

pub(crate) fn index_message(connection: &Connection, message: &MessageRecord) -> Result<()> {
    let existing: Option<i64> = connection
        .query_row(
            "SELECT id FROM search_keys WHERE conversation_id = ?1 AND message_id = ?2",
            params![message.conversation_id, message.message_id],
            |row| row.get(0),
        )
        .optional()?;
    let body = if message.deleted {
        String::new()
    } else {
        plain_text(&message.body_html)
    };
    if body.is_empty() {
        if let Some(id) = existing {
            connection.execute("DELETE FROM message_search WHERE rowid = ?1", [id])?;
            connection.execute("DELETE FROM search_keys WHERE id = ?1", [id])?;
        }
        return Ok(());
    }
    let id = match existing {
        Some(id) => {
            connection.execute("DELETE FROM message_search WHERE rowid = ?1", [id])?;
            id
        }
        None => {
            connection.execute(
                "INSERT INTO search_keys (conversation_id, message_id) VALUES (?1, ?2)",
                params![message.conversation_id, message.message_id],
            )?;
            connection.last_insert_rowid()
        }
    };
    connection.execute(
        "INSERT INTO message_search (rowid, body, sender) VALUES (?1, ?2, ?3)",
        params![id, body, message.sender_name.as_deref().unwrap_or_default()],
    )?;
    Ok(())
}

pub(crate) fn backfill(transaction: &Transaction<'_>) -> Result<()> {
    let mut statement = transaction.prepare(
        "SELECT conversation_id, message_id, sender_name, body_html FROM messages WHERE deleted = 0",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok(MessageRecord {
                conversation_id: row.get(0)?,
                message_id: row.get(1)?,
                sender_name: row.get(2)?,
                body_html: row.get(3)?,
                ..MessageRecord::default()
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for message in &rows {
        index_message(transaction, message)?;
    }
    Ok(())
}

/// Prefix-AND query. `from:name` filters on the sender. `None` when nothing searchable is left.
fn match_expression(query: &str) -> Option<String> {
    let terms: Vec<String> = query
        .split_whitespace()
        .filter_map(|word| {
            let (column, text) = match word.strip_prefix(SENDER_FILTER) {
                Some(rest) => ("sender : ", rest),
                None => ("", word),
            };
            text.chars()
                .any(char::is_alphanumeric)
                .then(|| format!("{column}\"{}\"*", text.replace('"', "\"\"")))
        })
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

impl Store {
    /// Newest first. Snippets wrap matches in `HIGHLIGHT_START` and `HIGHLIGHT_END`.
    pub fn search_messages(
        &self,
        query: &str,
        conversation_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        let Some(expression) = match_expression(query) else {
            return Ok(Vec::new());
        };
        let connection = self.lock()?;
        let mut statement = connection.prepare_cached(&format!(
            "SELECT k.conversation_id, k.message_id, m.sender_name, m.created_at,
                    snippet(message_search, 0, char({}), char({}), '...', {SNIPPET_TOKENS})
             FROM message_search
             JOIN search_keys k ON k.id = message_search.rowid
             JOIN messages m ON m.conversation_id = k.conversation_id AND m.message_id = k.message_id
             WHERE message_search MATCH ?1 AND (?2 IS NULL OR k.conversation_id = ?2)
             ORDER BY m.created_at DESC LIMIT ?3",
            HIGHLIGHT_START as u32, HIGHLIGHT_END as u32
        ))?;
        let hits = statement
            .query_map(params![expression, conversation_id, limit as i64], |row| {
                Ok(SearchHit {
                    conversation_id: row.get(0)?,
                    message_id: row.get(1)?,
                    sender_name: row.get(2)?,
                    created_at: from_millis(row.get(3)?),
                    snippet: row.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(hits)
    }

    pub fn search_conversations(&self, query: &str, limit: usize) -> Result<Vec<ConversationHit>> {
        let Some(expression) = match_expression(query).filter(|_| !query.contains(SENDER_FILTER))
        else {
            return Ok(Vec::new());
        };
        let connection = self.lock()?;
        let mut statement = connection.prepare_cached(
            "SELECT conversation_id, title FROM title_search WHERE title_search MATCH ?1 ORDER BY rank LIMIT ?2",
        )?;
        let hits = statement
            .query_map(params![expression, limit as i64], |row| {
                Ok(ConversationHit {
                    conversation_id: row.get(0)?,
                    title: row.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(hits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expression_quotes_and_prefixes_every_word() {
        assert_eq!(match_expression("bud pla").unwrap(), "\"bud\"* \"pla\"*");
        assert_eq!(
            match_expression("from:ada rep\"ort").unwrap(),
            "sender : \"ada\"* \"rep\"\"ort\"*"
        );
    }

    #[test]
    fn expression_drops_punctuation_only_words() {
        assert!(match_expression("- ::").is_none());
        assert!(match_expression("   ").is_none());
    }
}
