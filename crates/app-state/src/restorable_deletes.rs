use std::collections::HashSet;

pub const DELETED_TOMBSTONE: &str = "This message has been deleted.";
pub const UNDO_DELETE_LABEL: &str = "Undo";
pub const RESTORE_FAILED_NOTICE: &str = "Message restore failed. Please try again";

#[derive(Debug, Default, Clone)]
pub struct RestorableDeletes {
    messages: HashSet<(String, String)>,
}

impl RestorableDeletes {
    pub fn remember(&mut self, conversation_id: &str, message_id: &str) {
        self.messages
            .insert((conversation_id.to_owned(), message_id.to_owned()));
    }

    pub fn forget(&mut self, conversation_id: &str, message_id: &str) -> bool {
        self.messages
            .remove(&(conversation_id.to_owned(), message_id.to_owned()))
    }

    pub fn can_undo(
        &self,
        conversation_id: &str,
        message_id: &str,
        own: bool,
        deleted: bool,
    ) -> bool {
        own && deleted
            && self
                .messages
                .contains(&(conversation_id.to_owned(), message_id.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_own_messages_deleted_in_this_session_can_be_undone() {
        let mut deletes = RestorableDeletes::default();
        deletes.remember("19:a", "m1");
        assert!(deletes.can_undo("19:a", "m1", true, true));
        assert!(!deletes.can_undo("19:a", "m1", false, true));
        assert!(!deletes.can_undo("19:a", "m1", true, false));
        assert!(!deletes.can_undo("19:b", "m1", true, true));
        assert!(deletes.forget("19:a", "m1"));
        assert!(!deletes.can_undo("19:a", "m1", true, true));
        assert!(!deletes.forget("19:a", "m1"));
    }
}
