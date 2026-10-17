use std::collections::HashSet;

use store::Sidebar;

use crate::activity::Kind;
use crate::activity::feed::Feed;
use crate::sidebar_model::unread_chat_count;

pub fn taskbar_badge_count(sidebar: &Sidebar, feed: &Feed, adaptive_rail: bool) -> usize {
    let channel_ids: HashSet<&str> = sidebar
        .teams
        .iter()
        .flat_map(|team| team.channels.iter())
        .map(|channel| channel.id.as_str())
        .collect();
    let is_channel = |conversation_id: &str| channel_ids.contains(conversation_id);
    let activity = feed
        .unread_entries()
        .filter(|entry| entry.kind != Kind::Messages || is_channel(&entry.conversation_id))
        .count();
    let channels_with_mentions = if adaptive_rail {
        feed.unread_entries()
            .filter(|entry| entry.kind == Kind::Mention && is_channel(&entry.conversation_id))
            .map(|entry| entry.conversation_id.as_str())
            .collect::<HashSet<_>>()
            .len()
    } else {
        0
    };
    activity + unread_chat_count(&sidebar.chats) as usize + channels_with_mentions
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use store::{ChannelRecord, ChatRecord, SidebarTeam, TeamRecord};

    use super::*;
    use crate::activity::Entry;

    fn sidebar() -> Sidebar {
        let chat = |id: &str, unread: bool, muted: bool| ChatRecord {
            id: id.into(),
            unread,
            muted,
            ..Default::default()
        };
        let channel = |id: &str| ChannelRecord {
            id: id.into(),
            team_id: "team".into(),
            name: id.into(),
            membership_type: None,
            last_message_at: None,
            unread: true,
        };
        Sidebar {
            chats: vec![
                chat("chat-unread", true, false),
                chat("chat-muted", true, true),
                chat("chat-read", false, false),
            ],
            teams: vec![SidebarTeam {
                team: TeamRecord {
                    id: "team".into(),
                    name: "Team".into(),
                },
                channels: vec![channel("channel-a"), channel("channel-b")],
                hidden: false,
                hidden_channel_ids: vec!["channel-b".into()],
                notifications: Default::default(),
            }],
        }
    }

    fn feed(entries: &[(&str, Kind, bool)]) -> Feed {
        let mut feed = Feed::load(Vec::new());
        for (conversation_id, kind, read) in entries {
            feed.add(Entry {
                id: 0,
                conversation_id: (*conversation_id).into(),
                kind: *kind,
                message_id: String::new(),
                actors: Vec::new(),
                preview: String::new(),
                glyphs: Vec::new(),
                count: 1,
                updated_at: Utc::now(),
                read: *read,
            });
        }
        feed
    }

    #[test]
    fn unread_channel_posts_without_activity_do_not_count() {
        assert_eq!(taskbar_badge_count(&sidebar(), &feed(&[]), true), 1);
    }

    #[test]
    fn muted_chats_never_count_as_chats() {
        let mentioned = feed(&[("chat-muted", Kind::Mention, false)]);
        assert_eq!(taskbar_badge_count(&sidebar(), &mentioned, false), 2);
    }

    #[test]
    fn chat_message_rows_are_counted_once_by_the_chat() {
        let rows = feed(&[
            ("chat-unread", Kind::Messages, false),
            ("chat-unread", Kind::Reaction, false),
            ("channel-a", Kind::Messages, false),
            ("channel-a", Kind::Reaction, true),
        ]);
        assert_eq!(taskbar_badge_count(&sidebar(), &rows, false), 3);
    }

    #[test]
    fn the_adaptive_rail_adds_one_per_channel_with_an_unread_mention() {
        let mentions = feed(&[
            ("channel-a", Kind::Mention, false),
            ("channel-a", Kind::Mention, false),
            ("channel-b", Kind::Mention, false),
            ("chat-unread", Kind::Mention, false),
        ]);
        assert_eq!(taskbar_badge_count(&sidebar(), &mentions, false), 5);
        assert_eq!(taskbar_badge_count(&sidebar(), &mentions, true), 7);
    }
}
