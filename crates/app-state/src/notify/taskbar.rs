use std::collections::HashSet;

use store::Sidebar;

use crate::activity::feed::Feed;
use crate::sidebar_model::unread_chat_count;

pub fn taskbar_badge_count(sidebar: &Sidebar, feed: &Feed, adaptive_rail: bool) -> usize {
    let channel_ids: HashSet<&str> = sidebar
        .teams
        .iter()
        .flat_map(|team| team.channels.iter())
        .map(|channel| channel.id.as_str())
        .collect();
    let channels_with_mentions = if adaptive_rail {
        feed.unread_entries()
            .filter(|entry| {
                entry.is_personal_mention() && channel_ids.contains(entry.conversation_id.as_str())
            })
            .map(|entry| entry.conversation_id.as_str())
            .collect::<HashSet<_>>()
            .len()
    } else {
        0
    };
    feed.bell_count() + unread_chat_count(&sidebar.chats) as usize + channels_with_mentions
}

#[cfg(test)]
mod tests {
    use store::{ChannelRecord, ChatRecord, SidebarTeam, TeamRecord};

    use super::*;
    use crate::activity::{Entry, Kind};

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
                rights: Default::default(),
                team: TeamRecord {
                    id: "team".into(),
                    name: "Team".into(),
                },
                channels: vec![channel("channel-a"), channel("channel-b")],
                hidden: false,
                hidden_channel_ids: vec!["channel-b".into()],
                notifications: Default::default(),
                mentions: Default::default(),
            }],
        }
    }

    fn feed(entries: &[(&str, Kind, bool)]) -> Feed {
        Feed::new(
            entries
                .iter()
                .enumerate()
                .map(|(index, (conversation_id, kind, read))| Entry {
                    id: index.to_string(),
                    conversation_id: (*conversation_id).into(),
                    kind: *kind,
                    subtype: "person".into(),
                    read: *read,
                    counts_for_bell: !*read,
                    ..Entry::default()
                })
                .collect(),
        )
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
    fn the_activity_term_is_the_bell() {
        let mut rows = feed(&[
            ("chat-unread", Kind::Reply, false),
            ("channel-a", Kind::Reaction, true),
            ("channel-a", Kind::Posted, false),
        ]);
        assert_eq!(taskbar_badge_count(&sidebar(), &rows, false), 3);
        rows = Feed::new(
            rows.entries()
                .iter()
                .cloned()
                .map(|entry| Entry {
                    counts_for_bell: false,
                    ..entry
                })
                .collect(),
        );
        assert_eq!(taskbar_badge_count(&sidebar(), &rows, false), 1);
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
