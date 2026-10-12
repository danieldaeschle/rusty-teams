use chatsvc::{MessageWindow, WindowDirection};
use chrono::{DateTime, Utc};
use store::MessageRecord;

use crate::engine::{SyncEngine, sorted};
use crate::error::Result;
use crate::mapping::message_record;
use crate::remote::Remote;

pub const WINDOW_PAGE_SIZE: usize = 20;

#[derive(Debug, Clone, Default)]
pub struct WindowPage {
    pub records: Vec<MessageRecord>,
    pub older_cursor: Option<String>,
    pub newer_cursor: Option<String>,
    pub oldest: Option<DateTime<Utc>>,
    pub newest: Option<DateTime<Utc>>,
}

impl<R: Remote> SyncEngine<R> {
    pub async fn fetch_chat_message(
        &self,
        chat_id: &str,
        message_id: &str,
    ) -> Result<Option<MessageRecord>> {
        let message = self.remote.chat_message(chat_id, message_id).await?;
        Ok(message_record(chat_id, &message))
    }

    pub async fn load_window(&self, chat_id: &str, anchor_id: &str) -> Result<WindowPage> {
        self.window_page(chat_id, anchor_id, WindowDirection::Bidirectional)
            .await
    }

    pub async fn load_window_page(
        &self,
        chat_id: &str,
        cursor: &str,
        direction: WindowDirection,
    ) -> Result<WindowPage> {
        self.window_page(chat_id, cursor, direction).await
    }

    async fn window_page(
        &self,
        chat_id: &str,
        position: &str,
        direction: WindowDirection,
    ) -> Result<WindowPage> {
        let MessageWindow {
            message_ids,
            older_cursor,
            newer_cursor,
        } = self
            .remote
            .chat_message_window(chat_id, position, direction, WINDOW_PAGE_SIZE)
            .await?;
        let mut records = Vec::with_capacity(message_ids.len());
        for answer in self
            .remote
            .chat_messages_by_id(chat_id, &message_ids)
            .await?
        {
            if let Some(record) = answer
                .ok()
                .and_then(|message| message_record(chat_id, &message))
            {
                records.push(record);
            }
        }
        let records = sorted(records);
        self.ingest(chat_id, records.clone())?;
        self.announce(chat_id, !records.is_empty());
        Ok(WindowPage {
            oldest: records.first().map(|record| record.created_at),
            newest: records.last().map(|record| record.created_at),
            records,
            older_cursor,
            newer_cursor,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpPlan {
    InPlace,
    LeaveWindow,
    LoadWindow,
}

pub fn jump_plan(
    window: Option<(DateTime<Utc>, DateTime<Utc>)>,
    target_created_at: Option<DateTime<Utc>>,
    oldest_loaded: Option<DateTime<Utc>>,
) -> JumpPlan {
    let Some(created_at) = target_created_at else {
        return JumpPlan::LoadWindow;
    };
    if window.is_some_and(|(oldest, newest)| oldest <= created_at && created_at <= newest) {
        JumpPlan::InPlace
    } else if oldest_loaded.is_none_or(|oldest_loaded| created_at >= oldest_loaded) {
        JumpPlan::LeaveWindow
    } else {
        JumpPlan::LoadWindow
    }
}

pub fn without_islands(
    mut records: Vec<MessageRecord>,
    oldest_loaded: Option<DateTime<Utc>>,
) -> Vec<MessageRecord> {
    if let Some(oldest_loaded) = oldest_loaded {
        records.retain(|record| record.created_at >= oldest_loaded);
    }
    records
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn at(minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 6, 9, minute, 0).unwrap()
    }

    fn record(minute: u32) -> MessageRecord {
        MessageRecord {
            message_id: minute.to_string(),
            created_at: at(minute),
            ..MessageRecord::default()
        }
    }

    #[test]
    fn jump_plan_and_island_filter() {
        assert_eq!(jump_plan(None, None, Some(at(5))), JumpPlan::LoadWindow);
        assert_eq!(
            jump_plan(None, Some(at(4)), Some(at(5))),
            JumpPlan::LoadWindow
        );
        assert_eq!(
            jump_plan(None, Some(at(5)), Some(at(5))),
            JumpPlan::LeaveWindow
        );
        assert_eq!(jump_plan(None, Some(at(1)), None), JumpPlan::LeaveWindow);
        let window = Some((at(10), at(20)));
        assert_eq!(
            jump_plan(window, Some(at(15)), Some(at(30))),
            JumpPlan::InPlace
        );
        assert_eq!(
            jump_plan(window, Some(at(25)), Some(at(5))),
            JumpPlan::LeaveWindow
        );
        assert_eq!(
            jump_plan(window, Some(at(9)), Some(at(30))),
            JumpPlan::LoadWindow
        );
        let kept = without_islands(vec![record(1), record(5), record(6)], Some(at(5)));
        let ids: Vec<&str> = kept
            .iter()
            .map(|record| record.message_id.as_str())
            .collect();
        assert_eq!(ids, ["5", "6"]);
        assert_eq!(without_islands(vec![record(1)], None).len(), 1);
    }
}
