use chrono::{DateTime, Utc};
use graph::{Attachment, Chat, Mention, Message, Reaction};
use store::{ChannelRecord, ChatRecord, MemberRecord, MessageRecord, TeamRecord};

use crate::adaptive_card::card_content_text;
use crate::links::EMPTY_LINKS;
use crate::markdown::plain_text_to_html;
use crate::preview::OwnedPreview;
use crate::stored::{AttachmentInfo, MentionInfo, QuoteInfo, ReactionInfo};

const NO_NAME: &str = "?";

pub fn chat_record(chat: &Chat, my_user_id: &str) -> Option<ChatRecord> {
    if chat
        .viewpoint
        .as_ref()
        .and_then(|viewpoint| viewpoint.is_hidden)
        == Some(true)
    {
        return None;
    }
    let others: Vec<&graph::Member> = chat
        .members
        .iter()
        .filter(|member| member.user_id.as_deref() != Some(my_user_id))
        .collect();
    let last_message_at = chat.last_message_time().or(chat.last_updated_date_time);
    let last_read_at = chat
        .viewpoint
        .as_ref()
        .and_then(|viewpoint| viewpoint.last_message_read_date_time);
    let last_from_me = chat
        .last_message_preview
        .as_ref()
        .and_then(|preview| preview.from.as_ref())
        .and_then(|sender| sender.user_id())
        == Some(my_user_id);
    let preview = chat
        .last_message_preview
        .as_ref()
        .map(OwnedPreview::from_graph);
    let last_event_system = preview.as_ref().is_some_and(|preview| preview.system);
    Some(ChatRecord {
        id: chat.id.clone(),
        kind: chat.chat_type.clone(),
        title: chat.title(my_user_id),
        member_summary: others
            .iter()
            .map(|member| member_name(member))
            .collect::<Vec<_>>()
            .join(", "),
        last_message_at,
        last_read_at,
        unread: !last_event_system
            && is_unread(chat.last_message_time(), last_read_at, last_from_me),
        muted: false,
        members: chat
            .members
            .iter()
            .map(|member| MemberRecord {
                user_id: member.user_id.clone(),
                display_name: member_name(member).to_owned(),
            })
            .collect(),
        last_message_deleted: preview.as_ref().is_some_and(|preview| preview.deleted),
        last_message_preview: preview.as_ref().and_then(|preview| preview.text.clone()),
        last_message_sender_id: preview
            .as_ref()
            .and_then(|preview| preview.sender_id.clone()),
        last_message_sender_name: preview
            .as_ref()
            .and_then(|preview| preview.sender_name.clone()),
        last_event_system,
    })
}

fn member_name(member: &graph::Member) -> &str {
    member
        .display_name
        .as_deref()
        .or(member.email.as_deref())
        .unwrap_or(NO_NAME)
}

fn is_unread(
    last_message: Option<DateTime<Utc>>,
    last_read: Option<DateTime<Utc>>,
    last_from_me: bool,
) -> bool {
    match (last_message, last_read) {
        (Some(message), Some(read)) => message > read && !last_from_me,
        (Some(_), None) => !last_from_me,
        _ => false,
    }
}

pub fn team_record(team: &graph::Team) -> TeamRecord {
    TeamRecord {
        id: team.id.clone(),
        name: team.display_name.clone(),
    }
}

pub fn channel_record(team_id: &str, channel: &graph::Channel) -> ChannelRecord {
    ChannelRecord {
        id: channel.id.clone(),
        team_id: team_id.to_owned(),
        name: channel.display_name.clone(),
        membership_type: channel.membership_type.clone(),
        last_message_at: None,
        unread: false,
    }
}

/// `None` for system events and messages without a creation time.
pub fn message_record(conversation_id: &str, message: &Message) -> Option<MessageRecord> {
    if message
        .message_type
        .as_deref()
        .is_some_and(|kind| kind != "message")
    {
        return None;
    }
    let created_at = message.created_date_time?;
    let deleted = message.is_deleted();
    let sender = message.from.as_ref();
    Some(MessageRecord {
        conversation_id: conversation_id.to_owned(),
        message_id: message.id.clone(),
        reply_to_id: message.reply_to_id.clone(),
        sender_id: sender
            .and_then(|sender| sender.user_id())
            .map(str::to_owned),
        sender_name: sender
            .and_then(|sender| sender.display_name())
            .map(str::to_owned),
        sender_application_id: sender
            .and_then(|sender| sender.application_id())
            .map(str::to_owned),
        created_at,
        edited_at: message.last_edited_date_time,
        deleted,
        body_html: if deleted {
            String::new()
        } else {
            body_html(message)
        },
        attachments_json: to_json(
            &message
                .attachments
                .iter()
                .map(attachment_info)
                .collect::<Vec<_>>(),
        ),
        reactions_json: to_json(
            &message
                .reactions
                .iter()
                .map(reaction_info)
                .collect::<Vec<_>>(),
        ),
        mentions_json: to_json(
            &message
                .mentions
                .iter()
                .filter_map(mention_info)
                .collect::<Vec<_>>(),
        ),
        links_json: EMPTY_LINKS.to_owned(),
        subject: message
            .subject
            .as_deref()
            .map(str::trim)
            .filter(|subject| !subject.is_empty())
            .map(str::to_owned),
    })
}

pub fn flatten_thread(conversation_id: &str, thread: &Message) -> Vec<MessageRecord> {
    std::iter::once(thread)
        .chain(thread.replies.iter())
        .filter_map(|message| message_record(conversation_id, message))
        .collect()
}

fn body_html(message: &Message) -> String {
    let Some(body) = message.body.as_ref() else {
        return String::new();
    };
    let content = body.content.as_deref().unwrap_or_default();
    if body.content_type.eq_ignore_ascii_case("html") {
        content.to_owned()
    } else {
        plain_text_to_html(content.trim())
    }
}

pub(crate) fn attachment_info(attachment: &Attachment) -> AttachmentInfo {
    let is_card = attachment
        .content_type
        .as_deref()
        .is_some_and(|kind| kind.contains("card"));
    let text = match (&attachment.content, is_card) {
        (Some(content), true) => card_content_text(content),
        _ => attachment
            .name
            .as_ref()
            .map(|name| format!("[attachment: {name}]")),
    };
    AttachmentInfo {
        content_type: attachment.content_type.clone(),
        name: attachment.name.clone(),
        url: attachment.content_url.clone(),
        text,
        size: attachment.content.as_deref().and_then(content_size),
        quote: attachment
            .content
            .as_deref()
            .filter(|_| attachment.content_type.as_deref() == Some("messageReference"))
            .and_then(quote_info),
        id: attachment.id.clone(),
        content: attachment
            .content
            .clone()
            .filter(|_| is_card || attachment.content_type.as_deref() == Some("messageReference")),
    }
}

fn content_size(content: &str) -> Option<u64> {
    serde_json::from_str::<serde_json::Value>(content)
        .ok()?
        .get("size")?
        .as_u64()
}

fn quote_info(content: &str) -> Option<QuoteInfo> {
    let value: serde_json::Value = serde_json::from_str(content).ok()?;
    Some(QuoteInfo {
        message_id: value.get("messageId")?.as_str()?.to_owned(),
        preview: value
            .get("messagePreview")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        sender_name: value
            .pointer("/messageSender/user/displayName")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
    })
}

fn reaction_info(reaction: &Reaction) -> ReactionInfo {
    ReactionInfo {
        reaction_type: reaction.reaction_type.clone(),
        user_id: reaction
            .user
            .as_ref()
            .and_then(|sender| sender.user_id())
            .map(str::to_owned),
        user_name: reaction
            .user
            .as_ref()
            .and_then(|sender| sender.display_name())
            .map(str::to_owned),
        created_at: reaction.created_date_time,
    }
}

fn mention_info(mention: &Mention) -> Option<MentionInfo> {
    let mentioned = mention.mentioned.as_ref();
    let name = mention.mention_text.clone().or_else(|| {
        mentioned
            .and_then(|sender| sender.display_name())
            .map(str::to_owned)
    })?;
    Some(MentionInfo {
        user_id: mentioned
            .and_then(|sender| sender.user_id())
            .map(str::to_owned),
        name,
        id: mention.id,
        target_id: mentioned
            .and_then(|sender| sender.target_id())
            .map(str::to_owned),
        group: mentioned.is_some_and(|sender| sender.is_group()),
    })
}

fn to_json<T: serde::Serialize>(values: &[T]) -> String {
    serde_json::to_string(values).unwrap_or_else(|_| "[]".to_owned())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn chat_with_preview(preview: serde_json::Value) -> Chat {
        serde_json::from_value(json!({
            "id": "19:meeting@thread.v2",
            "chatType": "meeting",
            "viewpoint": {"lastMessageReadDateTime": "2026-10-06T09:00:00Z"},
            "lastMessagePreview": preview,
        }))
        .unwrap()
    }

    #[test]
    fn system_event_after_the_read_marker_is_not_unread() {
        let chat = chat_with_preview(json!({
            "id": "1",
            "messageType": "unknownFutureValue",
            "createdDateTime": "2026-10-06T10:00:00Z",
            "from": null,
        }));
        let record = chat_record(&chat, "me").unwrap();
        assert!(!record.unread);
        assert!(record.last_event_system);
    }

    #[test]
    fn regular_message_after_the_read_marker_is_unread() {
        let chat = chat_with_preview(json!({
            "id": "1",
            "messageType": "message",
            "createdDateTime": "2026-10-06T10:00:00Z",
            "from": {"user": {"id": "other"}},
        }));
        let record = chat_record(&chat, "me").unwrap();
        assert!(record.unread);
        assert!(!record.last_event_system);
    }

    fn record_with_subject(subject: serde_json::Value) -> Option<String> {
        let message: Message = serde_json::from_value(json!({
            "id": "1",
            "createdDateTime": "2026-10-06T09:00:00Z",
            "subject": subject,
        }))
        .unwrap();
        message_record("c", &message).unwrap().subject
    }

    #[test]
    fn subject_is_kept_trimmed_and_blank_ones_are_dropped() {
        assert_eq!(
            record_with_subject(json!("  Release plan ")).as_deref(),
            Some("Release plan")
        );
        assert_eq!(record_with_subject(json!("   ")), None);
        assert_eq!(record_with_subject(json!(null)), None);
    }

    fn mentions_of(mentioned: serde_json::Value) -> Vec<MentionInfo> {
        let message: Message = serde_json::from_value(json!({
            "id": "1",
            "createdDateTime": "2026-10-06T09:00:00Z",
            "mentions": [{"id": 0, "mentionText": "Target", "mentioned": mentioned}],
        }))
        .unwrap();
        message_record("c", &message)
            .map(|record| crate::stored::mentions(&record))
            .unwrap()
    }

    #[test]
    fn channel_team_and_tag_mentions_are_group_mentions() {
        for mentioned in [
            json!({"conversation": {"id": "19:chan@thread.tacv2", "conversationIdentityType": "channel"}}),
            json!({"conversation": {"id": "team-1", "conversationIdentityType": "team"}}),
            json!({"tag": {"id": "tag-1"}}),
        ] {
            let mentions = mentions_of(mentioned);
            assert!(mentions[0].group);
            assert_eq!(mentions[0].user_id, None);
            assert!(mentions[0].target_id.is_some());
        }
    }

    #[test]
    fn user_and_application_mentions_are_not_group_mentions() {
        for mentioned in [
            json!({"user": {"id": "u1"}}),
            json!({"application": {"id": "bot-1"}}),
        ] {
            assert!(!mentions_of(mentioned)[0].group);
        }
    }
}
