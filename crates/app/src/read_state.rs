#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadTrigger {
    Open,
    Activation,
    Incoming,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ReadPlan {
    pub clear_divider: bool,
}

pub fn plan_read(
    window_active: bool,
    chat_unread: bool,
    trigger: ReadTrigger,
) -> Option<ReadPlan> {
    if !window_active || !chat_unread {
        return None;
    }
    Some(ReadPlan {
        clear_divider: trigger != ReadTrigger::Open,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    struct LocalState {
        unread_counts: HashMap<String, u32>,
        unread: bool,
        first_unread: Option<String>,
        markers_sent: u32,
    }

    impl LocalState {
        fn apply(&mut self, window_active: bool, trigger: ReadTrigger) {
            let Some(plan) = plan_read(window_active, self.unread, trigger) else {
                return;
            };
            self.unread = false;
            self.unread_counts.remove("chat");
            if plan.clear_divider {
                self.first_unread = None;
            }
            self.markers_sent += 1;
        }
    }

    fn unread_chat() -> LocalState {
        LocalState {
            unread_counts: HashMap::from([("chat".to_owned(), 1)]),
            unread: true,
            first_unread: Some("message".to_owned()),
            markers_sent: 0,
        }
    }

    #[test]
    fn inactive_window_keeps_badge_and_divider() {
        let mut state = unread_chat();
        state.apply(false, ReadTrigger::Incoming);
        assert!(state.unread);
        assert_eq!(state.unread_counts.get("chat"), Some(&1));
        assert!(state.first_unread.is_some());
        assert_eq!(state.markers_sent, 0);
    }

    #[test]
    fn activation_clears_badge_and_divider_together() {
        let mut state = unread_chat();
        state.apply(false, ReadTrigger::Incoming);
        state.apply(true, ReadTrigger::Activation);
        assert!(!state.unread);
        assert!(state.unread_counts.is_empty());
        assert!(state.first_unread.is_none());
        assert_eq!(state.markers_sent, 1);
    }

    #[test]
    fn opening_an_unread_chat_keeps_the_divider() {
        let mut state = unread_chat();
        state.apply(true, ReadTrigger::Open);
        assert!(!state.unread);
        assert!(state.first_unread.is_some());
    }

    #[test]
    fn incoming_message_in_active_window_clears_divider() {
        let mut state = unread_chat();
        state.apply(true, ReadTrigger::Incoming);
        assert!(state.first_unread.is_none());
    }

    #[test]
    fn already_read_chat_sends_no_marker() {
        let mut state = unread_chat();
        state.apply(true, ReadTrigger::Activation);
        state.apply(true, ReadTrigger::Activation);
        assert_eq!(state.markers_sent, 1);
    }
}
