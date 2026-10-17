use std::collections::{HashMap, HashSet};

use chatsvc::{ConversationState, MentionActivity, MentionKind};
use store::ChannelMentions;

/// Teams counts a mention only while its channel is unread, it is newer than the read horizon and not read.
pub fn channel_mentions(
    activities: &[MentionActivity],
    states: &[ConversationState],
    my_user_id: &str,
    app_horizon_ms: i64,
) -> Vec<(String, ChannelMentions)> {
    let states: HashMap<&str, &ConversationState> = states
        .iter()
        .map(|state| (state.id.as_str(), state))
        .collect();
    let mut counted_activity_ids = HashSet::new();
    let mut mentions: HashMap<String, ChannelMentions> = HashMap::new();
    for activity in activities {
        if activity.is_read || !counted_activity_ids.insert(activity.activity_id.as_str()) {
            continue;
        }
        let Some(state) = states.get(activity.source_thread_id.as_str()) else {
            continue;
        };
        let horizon_ms = state.read.consumed_until_ms.max(app_horizon_ms);
        if activity.timestamp_ms <= horizon_ms
            || !state.read.is_unread_after(my_user_id, app_horizon_ms)
        {
            continue;
        }
        let counts = mentions
            .entry(activity.source_thread_id.clone())
            .or_default();
        match activity.kind {
            MentionKind::Personal => counts.personal += 1,
            MentionKind::Channel => counts.channel += 1,
            MentionKind::Team => counts.team += 1,
        }
    }
    let mut mentions: Vec<(String, ChannelMentions)> = mentions.into_iter().collect();
    mentions.sort_by(|left, right| left.0.cmp(&right.0));
    mentions
}

#[cfg(test)]
mod tests {
    use chatsvc::ReadHorizon;

    use super::*;

    const ME: &str = "8:orgid:me";

    fn state(id: &str, consumed_until_ms: i64, last_message_ms: i64) -> ConversationState {
        ConversationState {
            id: id.to_owned(),
            muted: false,
            hidden: None,
            read: ReadHorizon {
                consumed_until_ms,
                last_message_ms,
                last_message_from: Some("8:orgid:someone".to_owned()),
                ..ReadHorizon::default()
            },
            notification_settings: None,
            history_cleared_ms: None,
        }
    }

    fn activity(id: &str, thread: &str, kind: MentionKind, timestamp_ms: i64) -> MentionActivity {
        MentionActivity {
            activity_id: id.to_owned(),
            kind,
            source_thread_id: thread.to_owned(),
            source_message_id: None,
            timestamp_ms,
            is_read: false,
        }
    }

    fn counts(personal: u32, channel: u32, team: u32) -> ChannelMentions {
        ChannelMentions {
            personal,
            channel,
            team,
        }
    }

    #[test]
    fn counts_unread_mentions_after_the_horizon_per_kind() {
        let states = [state("19:a", 100, 500), state("19:b", 100, 500)];
        let activities = [
            activity("1", "19:a", MentionKind::Personal, 200),
            activity("2", "19:a", MentionKind::Personal, 300),
            activity("2", "19:a", MentionKind::Personal, 300),
            activity("3", "19:a", MentionKind::Channel, 300),
            activity("4", "19:b", MentionKind::Team, 400),
            activity("5", "19:a", MentionKind::Personal, 50),
            MentionActivity {
                is_read: true,
                ..activity("6", "19:b", MentionKind::Personal, 400)
            },
            activity("7", "19:unknown", MentionKind::Personal, 400),
        ];
        assert_eq!(
            channel_mentions(&activities, &states, ME, 0),
            [
                ("19:a".to_owned(), counts(2, 1, 0)),
                ("19:b".to_owned(), counts(0, 0, 1)),
            ]
        );
    }

    #[test]
    fn a_read_channel_has_no_mentions() {
        let states = [state("19:a", 600, 500)];
        let activities = [activity("1", "19:a", MentionKind::Personal, 550)];
        assert!(channel_mentions(&activities, &states, ME, 0).is_empty());
    }

    #[test]
    fn mark_all_as_read_hides_older_mentions() {
        let states = [state("19:a", 100, 900)];
        let activities = [
            activity("1", "19:a", MentionKind::Personal, 300),
            activity("2", "19:a", MentionKind::Personal, 800),
        ];
        assert_eq!(
            channel_mentions(&activities, &states, ME, 500),
            [("19:a".to_owned(), counts(1, 0, 0))]
        );
        assert!(channel_mentions(&activities, &states, ME, 950).is_empty());
    }
}
