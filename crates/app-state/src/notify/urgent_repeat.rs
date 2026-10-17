use chrono::{DateTime, Duration, Utc};

use super::rules::Incoming;

pub const REPEAT_INTERVAL_MINUTES: i64 = 2;
pub const REPEAT_WINDOW_MINUTES: i64 = 20;
const REPEAT_COUNT: i64 = REPEAT_WINDOW_MINUTES / REPEAT_INTERVAL_MINUTES;

struct Pending {
    incoming: Incoming,
    repeats_sent: i64,
}

/// Teams notifies again every 2 minutes for 20 minutes until an urgent message is read.
#[derive(Default)]
pub struct UrgentRepeats {
    pending: Vec<Pending>,
}

impl UrgentRepeats {
    pub fn track(&mut self, incoming: &Incoming, now: DateTime<Utc>) {
        let already_tracked = self.pending.iter().any(|pending| {
            pending.incoming.conversation_id == incoming.conversation_id
                && pending.incoming.message_id == incoming.message_id
        });
        if !incoming.is_urgent() || already_tracked || expired(incoming.created_at, now) {
            return;
        }
        self.pending.push(Pending {
            incoming: incoming.clone(),
            repeats_sent: 0,
        });
    }

    pub fn stop_conversation(&mut self, conversation_id: &str) {
        self.pending
            .retain(|pending| pending.incoming.conversation_id != conversation_id);
    }

    pub fn stop_read(&mut self, read_until: impl Fn(&str) -> Option<DateTime<Utc>>) {
        self.pending.retain(|pending| {
            read_until(&pending.incoming.conversation_id)
                .is_none_or(|read_at| read_at < pending.incoming.created_at)
        });
    }

    pub fn due(&mut self, now: DateTime<Utc>) -> Vec<Incoming> {
        let mut repeats = Vec::new();
        self.pending.retain_mut(|pending| {
            if expired(pending.incoming.created_at, now) {
                return false;
            }
            let slot = elapsed_slots(pending.incoming.created_at, now).min(REPEAT_COUNT);
            if slot > pending.repeats_sent {
                pending.repeats_sent = slot;
                repeats.push(pending.incoming.clone());
            }
            pending.repeats_sent < REPEAT_COUNT
        });
        repeats
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

fn elapsed_slots(created_at: DateTime<Utc>, now: DateTime<Utc>) -> i64 {
    let elapsed = now - created_at;
    if elapsed < Duration::zero() {
        return 0;
    }
    elapsed.num_seconds() / (REPEAT_INTERVAL_MINUTES * 60)
}

fn expired(created_at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    now - created_at >= Duration::minutes(REPEAT_WINDOW_MINUTES + REPEAT_INTERVAL_MINUTES)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use store::MessageImportance;

    use super::*;
    use crate::notify::rules::{ChatKind, Preview};

    fn sent_at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 17, 9, 0, 0).unwrap()
    }

    fn minutes(count: i64) -> DateTime<Utc> {
        sent_at() + Duration::minutes(count)
    }

    fn incoming(importance: MessageImportance) -> Incoming {
        Incoming {
            conversation_id: "chat".into(),
            message_id: "m1".into(),
            kind: ChatKind::Direct,
            chat_title: "Lea".into(),
            sender_id: Some("lea".into()),
            sender_name: "Lea".into(),
            preview: Preview::Text("Server down".into()),
            mentions_me: false,
            muted: false,
            signals: Default::default(),
            chat: Default::default(),
            created_at: sent_at(),
            importance,
        }
    }

    fn tracked() -> UrgentRepeats {
        let mut repeats = UrgentRepeats::default();
        repeats.track(&incoming(MessageImportance::Urgent), sent_at());
        repeats
    }

    #[test]
    fn repeats_every_two_minutes_for_twenty_minutes() {
        let mut repeats = tracked();
        let mut pings = Vec::new();
        for second in (0..=25 * 60).step_by(10) {
            let now = sent_at() + Duration::seconds(second);
            if !repeats.due(now).is_empty() {
                pings.push(second / 60);
            }
        }
        assert_eq!(pings, [2, 4, 6, 8, 10, 12, 14, 16, 18, 20]);
        assert!(repeats.is_empty());
    }

    #[test]
    fn only_urgent_messages_repeat() {
        let mut repeats = UrgentRepeats::default();
        repeats.track(&incoming(MessageImportance::High), sent_at());
        repeats.track(&incoming(MessageImportance::Normal), sent_at());
        assert!(repeats.is_empty());
    }

    #[test]
    fn tracking_twice_keeps_one_schedule() {
        let mut repeats = tracked();
        repeats.track(&incoming(MessageImportance::Urgent), minutes(1));
        assert_eq!(repeats.due(minutes(2)).len(), 1);
    }

    #[test]
    fn a_missed_tick_pings_once_not_in_a_burst() {
        let mut repeats = tracked();
        assert_eq!(repeats.due(minutes(9)).len(), 1);
        assert!(repeats.due(minutes(9)).is_empty());
        assert_eq!(repeats.due(minutes(10)).len(), 1);
    }

    #[test]
    fn opening_the_chat_stops_the_repeats() {
        let mut repeats = tracked();
        repeats.stop_conversation("chat");
        assert!(repeats.due(minutes(2)).is_empty());
        assert!(repeats.is_empty());
    }

    #[test]
    fn reading_on_another_device_stops_the_repeats() {
        let mut repeats = tracked();
        repeats.stop_read(|_| Some(minutes(-1)));
        assert!(!repeats.is_empty());
        repeats.stop_read(|_| Some(sent_at()));
        assert!(repeats.is_empty());
    }

    #[test]
    fn a_message_older_than_the_window_is_not_tracked() {
        let mut repeats = UrgentRepeats::default();
        repeats.track(&incoming(MessageImportance::Urgent), minutes(30));
        assert!(repeats.is_empty());
    }
}
