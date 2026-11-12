use chrono::{DateTime, Utc};
use teams_core::{ActivityItem, ActivitySnapshot};

use super::feed::{Actor, Entry, Feed, Kind};
use crate::rows::reaction_glyph;

const ORG_USER_PREFIX: &str = "8:orgid:";
const CHANNEL_THREAD_TYPES: [&str; 2] = ["topic", "space"];
const APP_TEMPLATES: [(&str, &str); 4] = [
    ("recapIsReady", "Meeting recap is ready"),
    ("recapIsReadyForFollowedMeetings", "Meeting recap is ready"),
    ("recapIsReadyV2", "Recap ready: {subject}"),
    (
        "recapIsReadyForFollowedMeetingsV2",
        "Recap ready: {subject}",
    ),
];

pub fn feed_of(snapshot: &ActivitySnapshot, seen_locally_ms: i64) -> Feed {
    Feed::new(
        snapshot
            .items
            .iter()
            .map(|item| {
                let read = snapshot.is_read(item);
                Entry {
                    read,
                    counts_for_bell: snapshot.counts_for_bell(item)
                        && item.timestamp_ms > seen_locally_ms,
                    ..entry_of(item)
                }
            })
            .collect(),
    )
}

fn entry_of(item: &ActivityItem) -> Entry {
    let kind = kind_of(item);
    let conversation_id = match (&item.meeting_thread_id, item.source_thread_id.is_empty()) {
        (Some(meeting_thread_id), true) => meeting_thread_id.clone(),
        _ => item.source_thread_id.clone(),
    };
    Entry {
        id: item.message_id.clone(),
        conversation_id,
        kind,
        activity_type: item.activity_type.clone(),
        subtype: item.activity_subtype.clone(),
        message_id: jump_target(item, kind),
        actors: vec![Actor {
            user_id: item
                .source_user_id
                .as_deref()
                .and_then(|user_id| user_id.strip_prefix(ORG_USER_PREFIX))
                .map(str::to_owned),
            name: item.source_user_name.clone(),
        }],
        preview: preview_of(item),
        glyphs: match kind {
            Kind::Reaction if !item.activity_subtype.is_empty() => {
                vec![reaction_glyph(&item.activity_subtype)]
            }
            _ => Vec::new(),
        },
        count: item.count.max(1),
        updated_at: DateTime::from_timestamp_millis(item.timestamp_ms).unwrap_or_else(Utc::now),
        read: false,
        counts_for_bell: false,
        source_label: item
            .source_app_name
            .clone()
            .or_else(|| item.thread_topic.clone()),
        link: item.tap_url.clone(),
        reply_id: channel_reply(item, kind),
        title: app_title(item),
        event_range: item
            .event_start_ms
            .zip(item.event_end_ms)
            .and_then(|(start_ms, end_ms)| {
                Some((
                    DateTime::from_timestamp_millis(start_ms)?,
                    DateTime::from_timestamp_millis(end_ms)?,
                ))
            }),
    }
}

fn app_title(item: &ActivityItem) -> Option<String> {
    let (_, template) = APP_TEMPLATES
        .iter()
        .find(|(subtype, _)| *subtype == item.activity_subtype)?;
    let mut title = String::new();
    let mut rest = *template;
    while let Some(open) = rest.find('{') {
        let close = open + rest[open..].find('}')?;
        title.push_str(&rest[..open]);
        title.push_str(item.template_parameters.get(&rest[open + 1..close])?.trim());
        rest = &rest[close + 1..];
    }
    title.push_str(rest);
    Some(title.trim().to_owned())
}

pub fn kind_of(item: &ActivityItem) -> Kind {
    match item.activity_type.as_str() {
        "mention"
        | "mentionInChat"
        | "mentionInCommunity"
        | "mentionInSubCommunity"
        | "communityMention" => Kind::Mention,
        "reply" | "replyToReply" | "replyInChat" | "communityReply" | "communityReplyToReply" => {
            Kind::Reply
        }
        "reaction"
        | "reactionInChat"
        | "like"
        | "likeInChat"
        | "reactionInCommunity"
        | "reactionInSubCommunity" => Kind::Reaction,
        "call" if item.activity_subtype == "missedCall" => Kind::MissedCall,
        "call" | "voicemail" | "callOrMeetup" => Kind::Call,
        "follow" => Kind::Posted,
        "teamMembershipChange" => Kind::Team,
        "msGraph" | "thirdParty" => Kind::App,
        _ => Kind::Other,
    }
}

fn channel_reply(item: &ActivityItem, kind: Kind) -> Option<String> {
    let target = jump_target(item, kind);
    item.source_message_id
        .clone()
        .filter(|reply_id| !target.is_empty() && *reply_id != target)
}

fn jump_target(item: &ActivityItem, kind: Kind) -> String {
    if matches!(kind, Kind::MissedCall | Kind::Call | Kind::Team | Kind::App) {
        return String::new();
    }
    let in_channel = item
        .thread_type
        .as_deref()
        .is_some_and(|thread_type| CHANNEL_THREAD_TYPES.contains(&thread_type));
    let root = item.source_reply_chain_id.as_ref().filter(|_| in_channel);
    root.or(item.source_message_id.as_ref())
        .cloned()
        .unwrap_or_default()
}

fn preview_of(item: &ActivityItem) -> String {
    let fixed = match item.preview_template.as_str() {
        "sentAFile" => "Sent a file",
        "sentAnImage" => "Sent an image",
        "sentACard" => "Sent a card",
        "previewUnavailable" if item.preview.is_empty() => "Preview Unavailable",
        "deleted" => "This message has been deleted.",
        "forwardedMessage" => "Forwarded message",
        "isMeeting" => "Teams Meeting",
        "replyWithQuote" if item.preview.is_empty() => "Replied with quote",
        _ => {
            return item
                .preview
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
        }
    };
    fixed.to_owned()
}

#[cfg(test)]
mod tests {
    use teams_core::ActivityHorizons;

    use super::*;

    fn item(activity_type: &str, subtype: &str) -> ActivityItem {
        ActivityItem {
            message_id: "1791368000001".into(),
            activity_id: "1".into(),
            activity_type: activity_type.into(),
            activity_subtype: subtype.into(),
            source_thread_id: "19:a@thread.tacv2".into(),
            source_message_id: Some("1791367000002".into()),
            source_reply_chain_id: Some("1791367000001".into()),
            source_user_id: Some("8:orgid:user-1".into()),
            source_user_name: "Ada Example".into(),
            preview: "Ship it".into(),
            preview_template: "hasText".into(),
            thread_type: Some("topic".into()),
            count: 1,
            timestamp_ms: 1791368000000,
            version_ms: 1791368000000,
            ..ActivityItem::default()
        }
    }

    #[test]
    fn a_preview_with_line_breaks_becomes_one_line() {
        let multiline = ActivityItem {
            preview: "Menu 09.11.-13.11. -\nPlease order\r\n until 9:30".into(),
            ..item("mention", "person")
        };
        assert_eq!(
            preview_of(&multiline),
            "Menu 09.11.-13.11. - Please order until 9:30"
        );
    }

    #[test]
    fn a_recap_activity_gets_the_app_title_and_meeting_time() {
        let mut recap = item("msGraph", "recapIsReadyForFollowedMeetingsV2");
        recap
            .template_parameters
            .insert("subject".into(), "Following: Sprint {Demo} ".into());
        recap.event_start_ms = Some(1793865600000);
        recap.event_end_ms = Some(1793869200000);
        let entry = entry_of(&recap);
        assert_eq!(
            entry.title.as_deref(),
            Some("Recap ready: Following: Sprint {Demo}")
        );
        let (start, end) = entry.event_range.unwrap();
        assert_eq!((end - start).num_minutes(), 60);
        assert_eq!(entry_of(&item("msGraph", "recapIsReadyV2")).title, None);
        assert_eq!(entry_of(&item("msGraph", "somethingElse")).title, None);
    }

    #[test]
    fn maps_the_teams_activity_types() {
        let cases = [
            ("mention", "person", Kind::Mention),
            ("mentionInChat", "everyone", Kind::Mention),
            ("replyToReply", "", Kind::Reply),
            ("reactionInChat", "heart", Kind::Reaction),
            ("call", "missedCall", Kind::MissedCall),
            ("call", "missedMeetingNudge", Kind::Call),
            ("follow", "channelNewMessage", Kind::Posted),
            ("teamMembershipChange", "addedToTeam", Kind::Team),
            ("msGraph", "recapIsReadyV2", Kind::App),
            ("unknownType", "", Kind::Other),
        ];
        for (activity_type, subtype, kind) in cases {
            assert_eq!(
                kind_of(&item(activity_type, subtype)),
                kind,
                "{activity_type}"
            );
        }
    }

    #[test]
    fn a_channel_reply_jumps_to_its_root_and_a_chat_to_the_message() {
        let channel = entry_of(&item("replyToReply", ""));
        assert_eq!(channel.message_id, "1791367000001");
        assert_eq!(channel.reply_id.as_deref(), Some("1791367000002"));
        let chat = entry_of(&ActivityItem {
            thread_type: Some("chat".into()),
            ..item("mentionInChat", "person")
        });
        assert_eq!(chat.message_id, "1791367000002");
        assert_eq!(chat.reply_id, None);
        assert_eq!(entry_of(&item("call", "missedCall")).message_id, "");
        assert_eq!(chat.actors[0].user_id.as_deref(), Some("user-1"));
    }

    #[test]
    fn a_recap_opens_the_meeting_chat_it_names() {
        let recap = entry_of(&ActivityItem {
            source_thread_id: String::new(),
            meeting_thread_id: Some("19:meeting_Zm9v@thread.v2".into()),
            tap_url: Some("https://teams.microsoft.com/l/entity/x".into()),
            ..item("msGraph", "recapIsReadyV2")
        });
        assert_eq!(recap.conversation_id, "19:meeting_Zm9v@thread.v2");
        assert_eq!(recap.message_id, "");
        assert_eq!(
            recap.link.as_deref(),
            Some("https://teams.microsoft.com/l/entity/x")
        );
    }

    #[test]
    fn previews_follow_the_teams_template() {
        let file = entry_of(&ActivityItem {
            preview_template: "sentAFile".into(),
            ..item("mention", "person")
        });
        assert_eq!(file.preview, "Sent a file");
        let recap = entry_of(&ActivityItem {
            preview: "You're following this meeting".into(),
            preview_template: "previewUnavailable".into(),
            ..item("msGraph", "recapIsReadyV2")
        });
        assert_eq!(recap.preview, "You're following this meeting");
        let unavailable = entry_of(&ActivityItem {
            preview: String::new(),
            preview_template: "previewUnavailable".into(),
            ..item("mention", "person")
        });
        assert_eq!(unavailable.preview, "Preview Unavailable");
        assert_eq!(entry_of(&item("mention", "person")).preview, "Ship it");
        assert_eq!(entry_of(&item("reactionInChat", "like")).glyphs.len(), 1);
    }

    #[test]
    fn read_and_bell_come_from_the_snapshot_and_the_local_seen_mark() {
        let snapshot = ActivitySnapshot {
            items: vec![
                ActivityItem {
                    message_id: "old".into(),
                    timestamp_ms: 100,
                    version_ms: 100,
                    ..item("mention", "person")
                },
                ActivityItem {
                    message_id: "new".into(),
                    timestamp_ms: 300,
                    version_ms: 300,
                    ..item("mention", "person")
                },
                ActivityItem {
                    message_id: "flagged".into(),
                    timestamp_ms: 400,
                    version_ms: 400,
                    read_flag: true,
                    ..item("mention", "person")
                },
            ],
            horizons: ActivityHorizons {
                seen_ms: 50,
                bookmark_ms: 150,
            },
        };
        let feed = feed_of(&snapshot, 0);
        let read: Vec<(&str, bool, bool)> = feed
            .entries()
            .iter()
            .map(|entry| (entry.id.as_str(), entry.read, entry.counts_for_bell))
            .collect();
        assert_eq!(
            read,
            [
                ("old", true, false),
                ("new", false, true),
                ("flagged", true, false)
            ]
        );
        assert_eq!(feed_of(&snapshot, 300).bell_count(), 0);
    }
}
