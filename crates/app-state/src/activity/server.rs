use chrono::{DateTime, Utc};
use teams_core::{ActivityItem, ActivitySnapshot};

use super::feed::{Actor, Entry, Feed, Kind};
use crate::rows::reaction_glyph;

const ORG_USER_PREFIX: &str = "8:orgid:";
const CHANNEL_THREAD_TYPES: [&str; 2] = ["topic", "space"];

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
    Entry {
        id: item.message_id.clone(),
        conversation_id: item.source_thread_id.clone(),
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
        source_label: item.source_app_name.clone().or_else(|| item.thread_topic.clone()),
    }
}

pub fn kind_of(item: &ActivityItem) -> Kind {
    match item.activity_type.as_str() {
        "mention" | "mentionInChat" | "mentionInCommunity" | "mentionInSubCommunity"
        | "communityMention" => Kind::Mention,
        "reply" | "replyToReply" | "replyInChat" | "communityReply" | "communityReplyToReply" => {
            Kind::Reply
        }
        "reaction" | "reactionInChat" | "like" | "likeInChat" | "reactionInCommunity"
        | "reactionInSubCommunity" => Kind::Reaction,
        "call" if item.activity_subtype == "missedCall" => Kind::MissedCall,
        "call" | "voicemail" | "callOrMeetup" => Kind::Call,
        "follow" => Kind::Posted,
        "teamMembershipChange" => Kind::Team,
        "msGraph" | "thirdParty" => Kind::App,
        _ => Kind::Other,
    }
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
        "previewUnavailable" => "Preview Unavailable",
        "deleted" => "This message has been deleted.",
        "forwardedMessage" => "Forwarded message",
        "isMeeting" => "Teams Meeting",
        "replyWithQuote" if item.preview.is_empty() => "Replied with quote",
        _ => return item.preview.clone(),
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
            assert_eq!(kind_of(&item(activity_type, subtype)), kind, "{activity_type}");
        }
    }

    #[test]
    fn a_channel_reply_jumps_to_its_root_and_a_chat_to_the_message() {
        let channel = entry_of(&item("replyToReply", ""));
        assert_eq!(channel.message_id, "1791367000001");
        let chat = entry_of(&ActivityItem {
            thread_type: Some("chat".into()),
            ..item("mentionInChat", "person")
        });
        assert_eq!(chat.message_id, "1791367000002");
        assert_eq!(entry_of(&item("call", "missedCall")).message_id, "");
        assert_eq!(chat.actors[0].user_id.as_deref(), Some("user-1"));
    }

    #[test]
    fn previews_follow_the_teams_template() {
        let file = entry_of(&ActivityItem {
            preview_template: "sentAFile".into(),
            ..item("mention", "person")
        });
        assert_eq!(file.preview, "Sent a file");
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
            [("old", true, false), ("new", false, true), ("flagged", true, false)]
        );
        assert_eq!(feed_of(&snapshot, 300).bell_count(), 0);
    }
}
