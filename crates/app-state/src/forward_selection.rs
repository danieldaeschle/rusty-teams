use gpui_kit::*;

use crate::app_state::{AppEvent, AppState};
use crate::rows::{Delivery, MessageRow};

pub const FORWARD_SELECTION_LIMIT: usize = 5;
pub const FORWARD_IMAGE_LIMIT: usize = 5;
pub const FORWARD_LABEL: &str = "Forward";
pub const THIS_MESSAGE_LABEL: &str = "This message";
pub const MULTIPLE_MESSAGES_LABEL: &str = "Multiple messages";
pub const SELECT_MESSAGE_LABEL: &str = "Select message";
pub const NEXT_LABEL: &str = "Next";
pub const CANCEL_LABEL: &str = "Cancel";
pub const LIMIT_REASON: &str = "You can only forward up to 5 messages at a time.";
pub const IMAGE_LIMIT_REASON: &str = "Messages with more than 5 images can't be forwarded.";
pub const UNDELIVERED_REASON: &str = "Undelivered messages can't be forwarded.";
pub const SCHEDULED_REASON: &str = "Scheduled messages can't be forwarded until they're sent.";
pub const DEFAULT_REASON: &str = "This message can't be forwarded.";

pub fn selected_count_label(count: usize) -> String {
    format!("{count} selected")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardSelection {
    conversation_id: String,
    message_ids: Vec<String>,
}

impl ForwardSelection {
    pub fn starting_with(conversation_id: &str, message_id: &str) -> Self {
        ForwardSelection {
            conversation_id: conversation_id.to_owned(),
            message_ids: vec![message_id.to_owned()],
        }
    }

    pub fn conversation_id(&self) -> &str {
        &self.conversation_id
    }

    pub fn message_ids(&self) -> &[String] {
        &self.message_ids
    }

    pub fn contains(&self, message_id: &str) -> bool {
        self.message_ids
            .iter()
            .any(|selected| selected == message_id)
    }

    pub fn len(&self) -> usize {
        self.message_ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.message_ids.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.message_ids.len() >= FORWARD_SELECTION_LIMIT
    }

    /// Removes a selected message or adds a new one; returns false when the limit refuses it.
    pub fn toggle(&mut self, message_id: &str) -> bool {
        if let Some(position) = self
            .message_ids
            .iter()
            .position(|selected| selected == message_id)
        {
            self.message_ids.remove(position);
            return true;
        }
        if self.is_full() {
            return false;
        }
        self.message_ids.push(message_id.to_owned());
        true
    }

    /// `rows` in timeline order; the result keeps only the selected rows, oldest first.
    pub fn ordered_ids<'a>(&self, rows: impl IntoIterator<Item = &'a MessageRow>) -> Vec<String> {
        rows.into_iter()
            .filter(|row| self.contains(&row.key))
            .map(|row| row.key.clone())
            .collect()
    }
}

/// Why `row` cannot be checked now, in Teams' words; `None` when it can.
pub fn block_reason(
    row: &MessageRow,
    selection: &ForwardSelection,
    selected_image_count: usize,
) -> Option<&'static str> {
    if selection.contains(&row.key) {
        return None;
    }
    match row.delivery {
        Delivery::Sending | Delivery::Failed(_) => return Some(UNDELIVERED_REASON),
        Delivery::Scheduled(_) => return Some(SCHEDULED_REASON),
        Delivery::Delivered => {}
    }
    if row.deleted || row.call.is_some() {
        return Some(DEFAULT_REASON);
    }
    if !row.images.is_empty() && selected_image_count + row.images.len() > FORWARD_IMAGE_LIMIT {
        return Some(IMAGE_LIMIT_REASON);
    }
    if selection.is_full() {
        return Some(LIMIT_REASON);
    }
    None
}

impl AppState {
    pub fn start_forward_selection(
        &mut self,
        conversation_id: &str,
        message_id: &str,
        cx: &mut Context<Self>,
    ) {
        if self.mode.read_only {
            self.raise_notice("Read-only mode: message not forwarded".to_owned(), None, cx);
            return;
        }
        self.forward_selection = Some(ForwardSelection::starting_with(conversation_id, message_id));
        cx.emit(AppEvent::ForwardSelection);
        cx.notify();
    }

    pub fn toggle_forward_selected(&mut self, message_id: &str, cx: &mut Context<Self>) {
        let Some(selection) = self.forward_selection.as_mut() else {
            return;
        };
        if selection.toggle(message_id) {
            cx.notify();
        }
    }

    pub fn cancel_forward_selection(&mut self, cx: &mut Context<Self>) {
        if self.forward_selection.take().is_some() {
            cx.emit(AppEvent::ForwardSelection);
            cx.notify();
        }
    }

    /// `ordered_ids` are the selected messages, oldest first. The selection stays until the forward is sent.
    pub fn forward_selected(&mut self, ordered_ids: &[String], cx: &mut Context<Self>) {
        let Some(conversation_id) = self
            .forward_selection
            .as_ref()
            .map(|selection| selection.conversation_id().to_owned())
        else {
            return;
        };
        if !ordered_ids.is_empty() {
            self.request_forward_messages(&conversation_id, ordered_ids, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_REASON, Delivery, FORWARD_SELECTION_LIMIT, ForwardSelection, IMAGE_LIMIT_REASON,
        LIMIT_REASON, MessageRow, SCHEDULED_REASON, UNDELIVERED_REASON, block_reason,
        selected_count_label,
    };
    use crate::rows::ScheduledState;
    use teams_core::ImageRef;

    fn row(key: &str) -> MessageRow {
        crate::pending_rows::pending_row(
            key.to_owned(),
            "chat".to_owned(),
            None,
            chrono::Utc::now(),
            &crate::scheduled_rows::scheduled_outgoing("<p>hello</p>"),
            Delivery::Delivered,
        )
    }

    fn image() -> ImageRef {
        ImageRef {
            id: "image".into(),
            url: "https://example.invalid/image.png".into(),
            width: None,
            height: None,
            alt: None,
        }
    }

    #[test]
    fn the_first_message_is_selected_and_toggling_adds_or_removes() {
        let mut selection = ForwardSelection::starting_with("chat", "m1");
        assert_eq!(selection.message_ids(), ["m1"]);
        assert!(selection.toggle("m2"));
        assert!(selection.contains("m2"));
        assert!(selection.toggle("m1"));
        assert_eq!(selection.message_ids(), ["m2"]);
        assert!(selection.toggle("m2"));
        assert!(selection.is_empty());
    }

    #[test]
    fn a_sixth_message_is_refused() {
        let mut selection = ForwardSelection::starting_with("chat", "m1");
        for id in ["m2", "m3", "m4", "m5"] {
            assert!(selection.toggle(id));
        }
        assert!(selection.is_full());
        assert!(!selection.toggle("m6"));
        assert_eq!(selection.len(), FORWARD_SELECTION_LIMIT);
        assert_eq!(block_reason(&row("m6"), &selection, 0), Some(LIMIT_REASON));
        assert_eq!(block_reason(&row("m3"), &selection, 0), None);
    }

    #[test]
    fn the_order_follows_the_timeline_not_the_clicks() {
        let mut selection = ForwardSelection::starting_with("chat", "m3");
        selection.toggle("m1");
        let rows = [row("m1"), row("m2"), row("m3")];
        assert_eq!(selection.ordered_ids(rows.iter()), ["m1", "m3"]);
    }

    #[test]
    fn unsent_scheduled_deleted_and_call_rows_cannot_be_checked() {
        let selection = ForwardSelection::starting_with("chat", "m1");
        let mut sending = row("a");
        sending.delivery = Delivery::Sending;
        assert_eq!(
            block_reason(&sending, &selection, 0),
            Some(UNDELIVERED_REASON)
        );
        let mut failed = row("b");
        failed.delivery = Delivery::Failed("offline".into());
        assert_eq!(
            block_reason(&failed, &selection, 0),
            Some(UNDELIVERED_REASON)
        );
        let mut scheduled = row("c");
        scheduled.delivery = Delivery::Scheduled(ScheduledState::Waiting);
        assert_eq!(
            block_reason(&scheduled, &selection, 0),
            Some(SCHEDULED_REASON)
        );
        let mut deleted = row("d");
        deleted.deleted = true;
        assert_eq!(block_reason(&deleted, &selection, 0), Some(DEFAULT_REASON));
    }

    #[test]
    fn images_over_the_combined_limit_block_the_row() {
        let selection = ForwardSelection::starting_with("chat", "m1");
        let mut pictures = row("p");
        pictures.images = vec![image(), image()];
        assert_eq!(block_reason(&pictures, &selection, 3), None);
        assert_eq!(
            block_reason(&pictures, &selection, 4),
            Some(IMAGE_LIMIT_REASON)
        );
    }

    #[test]
    fn the_footer_counts_the_selection() {
        assert_eq!(selected_count_label(3), "3 selected");
    }
}
