use std::collections::HashMap;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};

use crate::format::first_name;

pub const SEND_INTERVAL: Duration = Duration::from_secs(20);
pub const REMOTE_TIMEOUT: Duration = Duration::from_secs(22);
const UNKNOWN_NAME: &str = "Someone";
const LISTED_FIRST_NAMES: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Typer {
    user_id: String,
    name: String,
    last_seen: Instant,
    last_seen_at: DateTime<Utc>,
}

#[derive(Debug, Default)]
pub struct TypingState {
    conversations: HashMap<String, Vec<Typer>>,
}

impl TypingState {
    pub fn start(
        &mut self,
        conversation_id: &str,
        user_id: &str,
        name: &str,
        now: Instant,
        at: DateTime<Utc>,
    ) {
        let typers = self
            .conversations
            .entry(conversation_id.to_owned())
            .or_default();
        match typers.iter_mut().find(|typer| typer.user_id == user_id) {
            Some(typer) => {
                typer.last_seen = now;
                typer.last_seen_at = at;
                if !name.is_empty() {
                    typer.name = name.to_owned();
                }
            }
            None => typers.push(Typer {
                user_id: user_id.to_owned(),
                name: name.to_owned(),
                last_seen: now,
                last_seen_at: at,
            }),
        }
    }

    pub fn clear(&mut self, conversation_id: &str, user_id: &str) -> bool {
        self.remove_where(conversation_id, |typer| typer.user_id == user_id)
    }

    pub fn message_from(
        &mut self,
        conversation_id: &str,
        user_id: &str,
        sent_at: DateTime<Utc>,
    ) -> bool {
        self.remove_where(conversation_id, |typer| {
            typer.user_id == user_id && sent_at >= typer.last_seen_at
        })
    }

    pub fn expire(&mut self, now: Instant) -> bool {
        let before = self.conversations.values().map(Vec::len).sum::<usize>();
        for typers in self.conversations.values_mut() {
            typers.retain(|typer| now.duration_since(typer.last_seen) < REMOTE_TIMEOUT);
        }
        self.conversations.retain(|_, typers| !typers.is_empty());
        before != self.conversations.values().map(Vec::len).sum::<usize>()
    }

    pub fn next_expiry(&self) -> Option<Instant> {
        self.conversations
            .values()
            .flatten()
            .map(|typer| typer.last_seen + REMOTE_TIMEOUT)
            .min()
    }

    pub fn names(&self, conversation_id: &str) -> Vec<String> {
        self.conversations
            .get(conversation_id)
            .into_iter()
            .flatten()
            .map(|typer| match typer.name.trim() {
                "" => UNKNOWN_NAME.to_owned(),
                name => name.to_owned(),
            })
            .collect()
    }

    pub fn is_active(&self, conversation_id: &str) -> bool {
        self.conversations.contains_key(conversation_id)
    }

    fn remove_where(&mut self, conversation_id: &str, matches: impl Fn(&Typer) -> bool) -> bool {
        let Some(typers) = self.conversations.get_mut(conversation_id) else {
            return false;
        };
        let before = typers.len();
        typers.retain(|typer| !matches(typer));
        let changed = typers.len() != before;
        if typers.is_empty() {
            self.conversations.remove(conversation_id);
        }
        changed
    }
}

pub fn typing_label(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [only] => format!("{only} is typing"),
        [first, second] => {
            format!("{first} and {second} are typing")
        }
        [first, second, third] => format!(
            "{}, {}, and {} are typing",
            first_name(first),
            first_name(second),
            first_name(third)
        ),
        _ => format!(
            "{}, {}, and {} others are typing",
            first_name(&names[0]),
            first_name(&names[1]),
            names.len() - LISTED_FIRST_NAMES
        ),
    }
}

pub fn preview_label(names: &[String], one_on_one: bool) -> String {
    match names {
        [] => String::new(),
        [_] if one_on_one => "typing...".to_owned(),
        [only] => format!("{} is typing...", first_name(only)),
        _ => format!("{} people are typing...", names.len()),
    }
}

#[derive(Debug, Default)]
pub struct OutgoingTyping {
    sent_at: Option<Instant>,
}

impl OutgoingTyping {
    /// `Some(true)` asks for a Typing signal, `Some(false)` for a ClearTyping.
    pub fn on_text(&mut self, has_text: bool, now: Instant) -> Option<bool> {
        if !has_text {
            return self.sent_at.take().map(|_| false);
        }
        let due = self
            .sent_at
            .is_none_or(|sent_at| now.duration_since(sent_at) >= SEND_INTERVAL);
        due.then(|| {
            self.sent_at = Some(now);
            true
        })
    }

    pub fn stop(&mut self) -> bool {
        self.sent_at.take().is_some()
    }

    pub fn reset(&mut self) {
        self.sent_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| (*name).to_owned()).collect()
    }

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_790_000_000 + seconds, 0).unwrap()
    }

    #[test]
    fn label_has_four_forms() {
        assert_eq!(typing_label(&[]), "");
        assert_eq!(
            typing_label(&names(&["Ada Lovelace"])),
            "Ada Lovelace is typing"
        );
        assert_eq!(
            typing_label(&names(&["Ada Lovelace", "Alan Turing"])),
            "Ada Lovelace and Alan Turing are typing"
        );
        assert_eq!(
            typing_label(&names(&["Ada Lovelace", "Alan Turing", "Grace Hopper"])),
            "Ada, Alan, and Grace are typing"
        );
        assert_eq!(
            typing_label(&names(&[
                "Ada Lovelace",
                "Alan Turing",
                "Grace Hopper",
                "Linus T"
            ])),
            "Ada, Alan, and 2 others are typing"
        );
        assert_eq!(
            typing_label(&names(&["A B", "C D", "E F", "G H", "I J"])),
            "A, C, and 3 others are typing"
        );
    }

    #[test]
    fn preview_label_depends_on_chat_kind_and_count() {
        assert_eq!(preview_label(&names(&["Ada Lovelace"]), true), "typing...");
        assert_eq!(
            preview_label(&names(&["Ada Lovelace"]), false),
            "Ada is typing..."
        );
        assert_eq!(
            preview_label(&names(&["Ada Lovelace", "Alan Turing"]), false),
            "2 people are typing..."
        );
        assert_eq!(preview_label(&[], false), "");
    }

    #[test]
    fn start_refreshes_without_duplicating_and_keeps_order() {
        let mut state = TypingState::default();
        let now = Instant::now();
        state.start("c", "u1", "Ada", now, at(0));
        state.start("c", "u2", "Alan", now, at(0));
        state.start("c", "u1", "", now + Duration::from_secs(5), at(5));
        assert_eq!(state.names("c"), names(&["Ada", "Alan"]));
        assert!(state.is_active("c"));
        assert!(!state.is_active("other"));
    }

    #[test]
    fn blank_names_read_as_someone() {
        let mut state = TypingState::default();
        state.start("c", "u1", "  ", Instant::now(), at(0));
        assert_eq!(state.names("c"), names(&["Someone"]));
    }

    #[test]
    fn clear_removes_only_that_user() {
        let mut state = TypingState::default();
        let now = Instant::now();
        state.start("c", "u1", "Ada", now, at(0));
        state.start("c", "u2", "Alan", now, at(0));
        assert!(state.clear("c", "u1"));
        assert!(!state.clear("c", "u1"));
        assert_eq!(state.names("c"), names(&["Alan"]));
        assert!(state.clear("c", "u2"));
        assert!(!state.is_active("c"));
    }

    #[test]
    fn a_message_clears_its_sender_only_when_sent_after_the_typing() {
        let mut state = TypingState::default();
        let now = Instant::now();
        state.start("c", "u1", "Ada", now, at(10));
        assert!(!state.message_from("c", "u1", at(9)));
        assert!(!state.message_from("c", "u2", at(11)));
        assert!(!state.message_from("other", "u1", at(11)));
        assert!(state.message_from("c", "u1", at(11)));
        assert!(!state.is_active("c"));
    }

    #[test]
    fn typers_expire_after_the_timeout_and_a_refresh_extends_it() {
        let mut state = TypingState::default();
        let start = Instant::now();
        state.start("c", "u1", "Ada", start, at(0));
        state.start("c", "u2", "Alan", start + Duration::from_secs(10), at(10));
        assert_eq!(state.next_expiry(), Some(start + REMOTE_TIMEOUT));
        assert!(!state.expire(start + REMOTE_TIMEOUT - Duration::from_millis(1)));
        assert!(state.expire(start + REMOTE_TIMEOUT));
        assert_eq!(state.names("c"), names(&["Alan"]));
        assert_eq!(
            state.next_expiry(),
            Some(start + Duration::from_secs(10) + REMOTE_TIMEOUT)
        );
        assert!(state.expire(start + Duration::from_secs(32)));
        assert_eq!(state.next_expiry(), None);
        assert!(!state.expire(start + Duration::from_secs(100)));
    }

    #[test]
    fn outgoing_sends_at_most_every_twenty_seconds_while_typing() {
        let mut outgoing = OutgoingTyping::default();
        let start = Instant::now();
        assert_eq!(outgoing.on_text(true, start), Some(true));
        assert_eq!(
            outgoing.on_text(true, start + Duration::from_secs(19)),
            None
        );
        assert_eq!(
            outgoing.on_text(true, start + Duration::from_secs(20)),
            Some(true)
        );
    }

    #[test]
    fn outgoing_clears_once_when_the_text_empties() {
        let mut outgoing = OutgoingTyping::default();
        let start = Instant::now();
        assert_eq!(outgoing.on_text(false, start), None);
        outgoing.on_text(true, start);
        assert_eq!(outgoing.on_text(false, start), Some(false));
        assert_eq!(outgoing.on_text(false, start), None);
        assert_eq!(outgoing.on_text(true, start), Some(true));
    }

    #[test]
    fn outgoing_stop_reports_only_after_a_typing_was_sent() {
        let mut outgoing = OutgoingTyping::default();
        assert!(!outgoing.stop());
        outgoing.on_text(true, Instant::now());
        assert!(outgoing.stop());
        assert!(!outgoing.stop());
    }

    #[test]
    fn outgoing_reset_forgets_without_a_clear() {
        let mut outgoing = OutgoingTyping::default();
        let start = Instant::now();
        outgoing.on_text(true, start);
        outgoing.reset();
        assert_eq!(outgoing.on_text(false, start), None);
    }
}
