use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crate::views::message_appear::AppearKind;

const REPLACEMENT_GRACE: Duration = Duration::from_secs(15);
const SETTLE_MARGIN: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Origin {
    Server,
    Sending,
    Scheduled,
}

pub(super) struct Candidate<'a> {
    pub key: &'a str,
    pub own: bool,
    pub origin: Origin,
}

/// Marks messages that arrive while the chat is open, never the first paint, a sync, older history or the server copy of a sent message.
#[derive(Default)]
pub(super) struct AppearTracker {
    scope: String,
    was_live: bool,
    known: HashSet<String>,
    local: HashSet<String>,
    unmatched_sending: HashSet<String>,
    claimed: HashSet<String>,
    released_credits: Vec<Instant>,
    entering: HashMap<String, (AppearKind, Instant)>,
}

impl AppearTracker {
    pub(super) fn apply(
        &mut self,
        scope: &str,
        live: bool,
        reset: bool,
        rows: &[Candidate],
        now: Instant,
    ) {
        let animate = live && !reset && self.was_live && self.scope == scope;
        if self.scope != scope {
            self.scope = scope.to_owned();
            self.local.clear();
            self.unmatched_sending.clear();
            self.claimed.clear();
            self.released_credits.clear();
            self.entering.clear();
        }
        self.was_live = live;
        self.release_departed(rows, now);
        if animate {
            self.mark_new(rows, now);
        }
        self.known = rows.iter().map(|row| row.key.to_owned()).collect();
        self.local = rows
            .iter()
            .filter(|row| row.origin != Origin::Server)
            .map(|row| row.key.to_owned())
            .collect();
        self.entering.retain(|_, (kind, since)| {
            now.duration_since(*since) < kind.duration() + SETTLE_MARGIN
        });
    }

    pub(super) fn entering(&self, now: Instant) -> HashMap<String, AppearKind> {
        self.entering
            .iter()
            .filter(|(_, (kind, since))| now.duration_since(*since) < kind.duration() + SETTLE_MARGIN)
            .map(|(key, (kind, _))| (key.clone(), *kind))
            .collect()
    }

    fn release_departed(&mut self, rows: &[Candidate], now: Instant) {
        let present: HashSet<&str> = rows.iter().map(|row| row.key).collect();
        let departed: Vec<String> = self
            .local
            .iter()
            .filter(|key| !present.contains(key.as_str()))
            .cloned()
            .collect();
        for key in departed {
            self.unmatched_sending.remove(&key);
            if !self.claimed.remove(&key) {
                self.released_credits.push(now);
            }
        }
        self.released_credits
            .retain(|since| now.duration_since(*since) < REPLACEMENT_GRACE);
    }

    fn mark_new(&mut self, rows: &[Candidate], now: Instant) {
        let newest_known_server = rows
            .iter()
            .rposition(|row| row.origin == Origin::Server && self.known.contains(row.key));
        let first_new = newest_known_server.map_or(0, |index| index + 1);
        for row in &rows[first_new..] {
            if self.known.contains(row.key) {
                continue;
            }
            let kind = if row.own {
                AppearKind::Own
            } else {
                AppearKind::Incoming
            };
            match row.origin {
                Origin::Sending => {
                    self.unmatched_sending.insert(row.key.to_owned());
                }
                Origin::Scheduled => {}
                Origin::Server if row.own && self.take_replacement_credit() => continue,
                Origin::Server => {}
            }
            self.entering.insert(row.key.to_owned(), (kind, now));
        }
    }

    fn take_replacement_credit(&mut self) -> bool {
        if self.released_credits.pop().is_some() {
            return true;
        }
        let Some(key) = self.unmatched_sending.iter().next().cloned() else {
            return false;
        };
        self.unmatched_sending.remove(&key);
        self.claimed.insert(key);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server<'a>(key: &'a str, own: bool) -> Candidate<'a> {
        Candidate {
            key,
            own,
            origin: Origin::Server,
        }
    }

    fn sending(key: &str) -> Candidate<'_> {
        Candidate {
            key,
            own: true,
            origin: Origin::Sending,
        }
    }

    fn keys(tracker: &AppearTracker, now: Instant) -> Vec<(String, AppearKind)> {
        let mut entering: Vec<_> = tracker.entering(now).into_iter().collect();
        entering.sort_by(|left, right| left.0.cmp(&right.0));
        entering
    }

    fn opened(rows: &[Candidate], now: Instant) -> AppearTracker {
        let mut tracker = AppearTracker::default();
        tracker.apply("chat", true, false, rows, now);
        tracker
    }

    #[test]
    fn opening_a_chat_animates_nothing() {
        let now = Instant::now();
        let tracker = opened(&[server("a", false), server("b", true)], now);
        assert!(keys(&tracker, now).is_empty());
    }

    #[test]
    fn a_live_message_from_others_slides_in_once() {
        let now = Instant::now();
        let mut tracker = opened(&[server("a", false)], now);
        tracker.apply("chat", true, false, &[server("a", false), server("b", false)], now);
        assert_eq!(keys(&tracker, now), vec![("b".to_owned(), AppearKind::Incoming)]);
        let later = now + Duration::from_secs(1);
        tracker.apply("chat", true, false, &[server("a", false), server("b", false)], later);
        assert!(keys(&tracker, later).is_empty());
    }

    #[test]
    fn messages_fetched_while_syncing_or_right_after_do_not_animate() {
        let now = Instant::now();
        let mut tracker = AppearTracker::default();
        tracker.apply("chat", true, false, &[server("a", false)], now);
        tracker.apply("chat", false, false, &[server("a", false), server("b", false)], now);
        tracker.apply("chat", true, false, &[server("a", false), server("b", false), server("c", false)], now);
        assert!(keys(&tracker, now).is_empty());
    }

    #[test]
    fn older_history_above_the_known_messages_does_not_animate() {
        let now = Instant::now();
        let mut tracker = opened(&[server("c", false), server("d", false)], now);
        tracker.apply(
            "chat",
            true,
            false,
            &[server("a", false), server("b", false), server("c", false), server("d", false)],
            now,
        );
        assert!(keys(&tracker, now).is_empty());
    }

    #[test]
    fn switching_chats_drops_the_baseline() {
        let now = Instant::now();
        let mut tracker = opened(&[server("a", false)], now);
        tracker.apply("other", true, false, &[server("x", false), server("y", false)], now);
        assert!(keys(&tracker, now).is_empty());
    }

    #[test]
    fn a_sent_message_animates_as_pending_and_not_again_as_server_copy() {
        let now = Instant::now();
        let mut tracker = opened(&[server("a", false)], now);
        tracker.apply("chat", true, false, &[server("a", false), sending("local-1")], now);
        assert_eq!(keys(&tracker, now), vec![("local-1".to_owned(), AppearKind::Own)]);
        let later = now + Duration::from_secs(1);
        tracker.apply("chat", true, false, &[server("a", false), server("m1", true)], later);
        assert!(keys(&tracker, later).is_empty());
    }

    #[test]
    fn a_server_copy_arriving_before_the_pending_row_leaves_does_not_animate() {
        let now = Instant::now();
        let mut tracker = opened(&[server("a", false)], now);
        tracker.apply("chat", true, false, &[server("a", false), sending("local-1")], now);
        let later = now + Duration::from_secs(1);
        tracker.apply("chat", true, false, &[server("a", false), server("m1", true), sending("local-1")], later);
        tracker.apply("chat", true, false, &[server("a", false), server("m1", true)], later);
        tracker.apply("chat", true, false, &[server("a", false), server("m1", true), server("m2", true)], later);
        assert_eq!(keys(&tracker, later), vec![("m2".to_owned(), AppearKind::Own)]);
    }

    #[test]
    fn an_incoming_message_below_a_pending_row_still_animates() {
        let now = Instant::now();
        let mut tracker = opened(&[server("a", false), sending("local-1")], now);
        tracker.apply("chat", true, false, &[server("a", false), server("b", false), sending("local-1")], now);
        assert_eq!(keys(&tracker, now), vec![("b".to_owned(), AppearKind::Incoming)]);
    }

    #[test]
    fn a_reset_rebuild_never_animates() {
        let now = Instant::now();
        let mut tracker = opened(&[server("a", false)], now);
        tracker.apply("chat", true, true, &[server("a", false), server("b", false)], now);
        assert!(keys(&tracker, now).is_empty());
        tracker.apply("chat", true, false, &[server("a", false), server("b", false), server("c", false)], now);
        assert_eq!(keys(&tracker, now), vec![("c".to_owned(), AppearKind::Incoming)]);
    }

    #[test]
    fn edits_and_reactions_keep_the_key_and_do_not_animate() {
        let now = Instant::now();
        let rows = [server("a", false), server("b", true)];
        let mut tracker = opened(&rows, now);
        tracker.apply("chat", true, false, &rows, now);
        assert!(keys(&tracker, now).is_empty());
    }
}
