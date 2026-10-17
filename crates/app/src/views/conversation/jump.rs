use gpui_kit::*;

use super::ConversationView;
use crate::read_state::{JumpTarget, jump_target};
use crate::rows::Row;
use crate::views::shortcuts::keys_for_label;

pub(super) const JUMP_TO_NEWEST_LABEL: &str = "Jump to newest";
const JUMP_SHORTCUT_LABEL: &str = "Jump to last read or newest message";

pub(super) fn jump_to_newest_tooltip() -> SharedString {
    match keys_for_label(JUMP_SHORTCUT_LABEL) {
        Some(keys) => format!("{JUMP_TO_NEWEST_LABEL} ({keys})").into(),
        None => JUMP_TO_NEWEST_LABEL.into(),
    }
}

impl ConversationView {
    pub fn jump_to_last_read_or_newest(&mut self, cx: &mut Context<Self>) {
        if self.current.is_none() || self.in_feed() {
            return;
        }
        if self.is_detached() {
            self.leave_detached(cx);
            return;
        }
        let last_read_index = self
            .rows
            .borrow()
            .iter()
            .position(|row| matches!(row, Row::Message(message) if message.new_marker));
        let (top_item, scrolled_up) = {
            let scroller = self.scroller.read(cx);
            (scroller.top_item(), scroller.is_scrolled_up())
        };
        let Some(target) = jump_target(last_read_index, top_item, scrolled_up) else {
            return;
        };
        self.scroller.update(cx, |scroller, cx| match target {
            JumpTarget::Newest => scroller.scroll_to_end(cx),
            JumpTarget::LastRead(index) => {
                scroller.scroll_to_item(index, cx);
            }
        });
    }
}
