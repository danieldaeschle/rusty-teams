use chrono::{DateTime, Utc};
use graph::{Attachment, Chat, Mention, Message, Reaction};
use store::{ChannelRecord, ChatRecord, MemberRecord, MessageRecord, TeamRecord};

use crate::adaptive_card::card_content_text;
use crate::call_event::CallEvent;
use crate::forwarded::FORWARDED_CONTENT_TYPE;
use crate::links::EMPTY_LINKS;
use crate::markdown::plain_text_to_html;
use crate::preview::OwnedPreview;
use crate::stored::{AttachmentInfo, MentionInfo, QuoteInfo, ReactionInfo};

pub fn graph_hidden(chat: &Chat) -> bool {
    chat.viewpoint
        .as_ref()
        .and_then(|viewpoint| viewpoint.is_hidden)
        == Some(true)
}

pub fn chat_record(chat: &Chat, my_user_id: &str) -> ChatRecord {
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
    // Teams leaves calls out of the chat list line: the line of the message before the call stays.
    let last_event_call = preview.as_ref().is_some_and(|preview| preview.call);
    ChatRecord {
        id: chat.id.clone(),
        kind: chat.chat_type.clone(),
        title: chat.title(my_user_id),
        member_summary: others
            .iter()
            .map(|member| member.name())
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
                display_name: member.name().to_owned(),
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
        last_event_call,
    }
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

/// `None` for system events other than calls and messages without a creation time.
pub fn message_record(conversation_id: &str, message: &Message) -> Option<MessageRecord> {
    let is_system = message
        .message_type
        .as_deref()
        .is_some_and(|kind| kind != "message");
    let call_event = message
        .event_detail
        .as_ref()
        .and_then(CallEvent::from_graph)
        .filter(|_| is_system);
    if is_system && call_event.is_none() {
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
        body_html: if deleted || call_event.is_some() {
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
        call_event_json: call_event.as_ref().map(CallEvent::to_json),
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
            .filter(|_| {
                is_card
                    || matches!(
                        attachment.content_type.as_deref(),
                        Some("messageReference" | FORWARDED_CONTENT_TYPE)
                    )
            }),
        unavailable: false,
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
    use crate::call_event::{CallPhase, CallType};

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
        let record = chat_record(&chat, "me");
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
        let record = chat_record(&chat, "me");
        assert!(record.unread);
        assert!(!record.last_event_system);
    }

    #[test]
    fn a_member_without_name_or_email_is_an_unknown_user() {
        let chat: Chat = serde_json::from_value(json!({
            "id": "19:group@thread.v2",
            "chatType": "group",
            "members": [
                {"userId": "me", "displayName": "Daniel"},
                {"userId": "gone", "displayName": null, "email": ""},
                {"userId": "mail", "displayName": " ", "email": "anna@example.com"},
            ],
        }))
        .unwrap();
        let record = chat_record(&chat, "me");
        let names: Vec<&str> = record
            .members
            .iter()
            .map(|member| member.display_name.as_str())
            .collect();
        assert_eq!(names, ["Daniel", "Unknown User", "anna@example.com"]);
        assert_eq!(record.title, "Unknown User, anna@example.com");
        assert_eq!(graph::UNKNOWN_USER_NAME, store::UNKNOWN_USER_NAME);
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

    fn user(id: &str) -> serde_json::Value {
        json!({"user": {"id": id, "displayName": format!("Name {id}")}})
    }

    fn call_record(message_type: &str, detail: serde_json::Value) -> Option<MessageRecord> {
        let message: Message = serde_json::from_value(json!({
            "id": "1",
            "messageType": message_type,
            "createdDateTime": "2026-10-06T09:00:00Z",
            "from": null,
            "body": {"contentType": "html", "content": "<systemEventMessage/>"},
            "eventDetail": detail,
        }))
        .unwrap();
        message_record("c", &message)
    }

    fn ended(initiator: &str, participants: &[&str], duration: &str) -> serde_json::Value {
        json!({
            "@odata.type": "#microsoft.graph.callEndedEventMessageDetail",
            "callId": "call-1",
            "callEventType": "call",
            "callDuration": duration,
            "initiator": user(initiator),
            "callParticipants": participants
                .iter()
                .map(|id| json!({"participant": user(id)}))
                .collect::<Vec<_>>(),
        })
    }

    fn event_of(record: &MessageRecord) -> CallEvent {
        crate::call_event::call_event(record).unwrap()
    }

    #[test]
    fn call_ended_by_me_is_kept_with_duration_and_participants() {
        let record = call_record(
            "unknownFutureValue",
            ended("me", &["me", "other"], "PT12M3S"),
        )
        .unwrap();
        let event = event_of(&record);
        assert_eq!(event.phase, CallPhase::Ended);
        assert_eq!(event.call_type, CallType::Call);
        assert_eq!(event.call_id, "call-1");
        assert_eq!(event.duration_seconds, Some(723));
        assert_eq!(event.initiator_id.as_deref(), Some("me"));
        assert_eq!(event.initiator_name.as_deref(), Some("Name me"));
        assert_eq!(event.participant_ids, ["me", "other"]);
        assert!(!event.missed(Some("me")));
        assert!(record.body_html.is_empty());
        assert_eq!(record.sender_id, None);
    }

    #[test]
    fn call_ended_by_other_with_both_participants_is_not_missed() {
        let record = call_record(
            "systemEventMessage",
            ended("other", &["other", "me"], "PT46S"),
        )
        .unwrap();
        assert!(!event_of(&record).missed(Some("me")));
    }

    #[test]
    fn unanswered_incoming_call_is_missed() {
        let record = call_record("unknownFutureValue", ended("other", &["other"], "PT1M")).unwrap();
        let event = event_of(&record);
        assert!(event.missed(Some("me")));
        assert_eq!(event.title(true), "Missed call");
        assert_eq!(event.duration_seconds, Some(60));
        assert!(!event.missed(None));
    }

    #[test]
    fn guest_participant_counts_for_an_answered_call() {
        let mut detail = ended("other", &["other"], "PT2M");
        detail["callParticipants"]
            .as_array_mut()
            .unwrap()
            .push(json!({"participant": {"guest": {"id": "guest-1", "displayName": "Guest"}}}));
        let record = call_record("unknownFutureValue", detail).unwrap();
        let event = event_of(&record);
        assert_eq!(event.participant_ids, ["other", "guest-1"]);
        assert!(!event.missed(Some("me")));
    }

    #[test]
    fn unanswered_own_call_is_not_missed() {
        let record = call_record("unknownFutureValue", ended("me", &["me"], "PT1M")).unwrap();
        let event = event_of(&record);
        assert!(!event.missed(Some("me")));
        assert_eq!(event.plain_text(), "Call ended 1m");
    }

    #[test]
    fn call_started_is_kept_without_duration() {
        let record = call_record(
            "unknownFutureValue",
            json!({
                "@odata.type": "#microsoft.graph.callStartedEventMessageDetail",
                "callId": "call-1",
                "callEventType": "call",
                "initiator": {"user": {"id": "other", "displayName": null}},
            }),
        )
        .unwrap();
        let event = event_of(&record);
        assert_eq!(event.phase, CallPhase::Started);
        assert_eq!(event.initiator_id.as_deref(), Some("other"));
        assert_eq!(event.initiator_name, None);
        assert_eq!(event.duration_seconds, None);
        assert_eq!(event.plain_text(), "Call started");
    }

    #[test]
    fn meeting_ended_keeps_the_meeting_type() {
        let mut detail = ended("me", &["me", "a", "b"], "PT1H2M3S");
        detail["callEventType"] = json!("meeting");
        let event = event_of(&call_record("unknownFutureValue", detail).unwrap());
        assert_eq!(event.call_type, CallType::Meeting);
        assert_eq!(event.duration_seconds, Some(3723));
        assert_eq!(event.plain_text(), "Meeting ended 1h 2m 3s");
    }

    #[test]
    fn screen_share_is_treated_as_a_call() {
        let mut detail = ended("me", &["me", "a"], "PT5S");
        detail["callEventType"] = json!("screenShare");
        let event = event_of(&call_record("unknownFutureValue", detail).unwrap());
        assert_eq!(event.call_type, CallType::Call);
    }

    #[test]
    fn other_system_events_are_still_dropped() {
        let detail = json!({
            "@odata.type": "#microsoft.graph.membersAddedEventMessageDetail",
            "members": [],
        });
        assert!(call_record("unknownFutureValue", detail).is_none());
        assert!(call_record("systemEventMessage", json!(null)).is_none());
    }

    #[test]
    fn regular_messages_never_become_call_events() {
        let record = call_record("message", ended("me", &["me"], "PT1M")).unwrap();
        assert_eq!(record.call_event_json, None);
    }

    #[test]
    fn durations_parse_from_iso_8601() {
        use crate::call_event::parse_duration_seconds as parse;
        assert_eq!(parse("PT46S"), Some(46));
        assert_eq!(parse("PT1M"), Some(60));
        assert_eq!(parse("PT1M18S"), Some(78));
        assert_eq!(parse("PT1H2M3S"), Some(3723));
        assert_eq!(parse("PT40M19.26S"), Some(2419));
        assert_eq!(parse("P1DT1S"), Some(86_401));
        for garbage in ["", "P", "PT", "46S", "PT1X", "PTM", "PT12", "nonsense"] {
            assert_eq!(parse(garbage), None, "{garbage}");
        }
    }

    #[test]
    fn durations_format_like_teams() {
        use crate::call_event::format_duration as format;
        assert_eq!(format(3723), "1h 2m 3s");
        assert_eq!(format(723), "12m 3s");
        assert_eq!(format(45), "45s");
        assert_eq!(format(60), "1m");
        assert_eq!(format(3600), "1h");
        assert_eq!(format(3605), "1h 0m 5s");
        assert_eq!(format(0), "0s");
    }

    #[test]
    fn unparseable_duration_leaves_no_duration() {
        let record = call_record("unknownFutureValue", ended("me", &["me", "a"], "soon")).unwrap();
        let event = event_of(&record);
        assert_eq!(event.duration_seconds, None);
        assert_eq!(event.plain_text(), "Call ended");
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
