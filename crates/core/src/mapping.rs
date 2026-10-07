use chrono::{DateTime, Utc};
use graph::{Attachment, Chat, Mention, Message, Reaction};
use store::{ChannelRecord, ChatRecord, MemberRecord, MessageRecord, TeamRecord};

use crate::card::card_content_text;
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
        unread: is_unread(chat.last_message_time(), last_read_at, last_from_me),
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
    })
}

fn to_json<T: serde::Serialize>(values: &[T]) -> String {
    serde_json::to_string(values).unwrap_or_else(|_| "[]".to_owned())
}
