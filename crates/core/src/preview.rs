use graph::Message;
use serde_json::Value;
use store::{ChatPreview, MessageRecord};

use crate::call_event::{CallEvent, CallPhase, call_event};
use crate::external_image::is_giphy_url;
use crate::forwarded::FORWARDED_CONTENT_TYPE;
use crate::spans::{Span, html_to_spans};
use crate::stored::{AttachmentInfo, attachments};

const MAX_PREVIEW_CHARS: usize = 200;

pub const IMAGE_PREVIEW: &str = "Sent an image";
pub const GIF_PREVIEW: &str = "\u{1F4F7} GIF";
pub const VIDEO_PREVIEW: &str = "\u{1F3A5} Video";
pub const FILE_PREVIEW: &str = "Sent a file";
pub const CARD_PREVIEW: &str = "Sent a card";
pub const FORWARD_PREVIEW: &str = "Forwarded a message";
pub const AUDIO_PREVIEW: &str = "\u{1F399} Audio message";
pub const LOOP_PREVIEW: &str = "Loop component";
pub const RECORDING_STARTED_PREVIEW: &str = "Recording has started";
pub const RECORDING_STOPPED_PREVIEW: &str = "Recording has stopped";
pub const RECORDING_READY_PREVIEW: &str = "Recording is ready";
pub const TRANSCRIPT_PREVIEW: &str = "Transcript is available";
pub const USER_ADDED_PREVIEW: &str = "User added";
pub const USER_JOINED_PREVIEW: &str = "User joined";
const EVENT_PREVIEWS: [&str; 6] = [
    RECORDING_STARTED_PREVIEW,
    RECORDING_STOPPED_PREVIEW,
    RECORDING_READY_PREVIEW,
    TRANSCRIPT_PREVIEW,
    USER_ADDED_PREVIEW,
    USER_JOINED_PREVIEW,
];

const AUDIO_CARD_TYPE: &str = "application/vnd.microsoft.card.audio";
const LOOP_CARD_TYPE: &str = "application/vnd.microsoft.card.fluidEmbedCard";
const FILE_CONTENT_TYPE: &str = "reference";
const RECORDING_TYPE: &str = "#microsoft.graph.callRecordingEventMessageDetail";
const TRANSCRIPT_TYPE: &str = "#microsoft.graph.callTranscriptEventMessageDetail";
const MEMBERS_ADDED_TYPE: &str = "#microsoft.graph.membersAddedEventMessageDetail";
const MEMBERS_JOINED_TYPE: &str = "#microsoft.graph.membersJoinedEventMessageDetail";

#[derive(Default)]
struct BodyParts {
    text: String,
    images: usize,
    gifs: usize,
}

/// Plain text of a message body as the chat list shows it: emoji as glyphs, mentions by name, reply quotes and images dropped.
pub fn preview_text(html: &str) -> Option<String> {
    let mut parts = BodyParts::default();
    collect(&html_to_spans(html), &mut parts);
    collapse(&parts.text)
}

pub fn is_event_preview(text: &str) -> bool {
    EVENT_PREVIEWS.contains(&text)
}

/// The chat list line for a message, with the Teams labels for bodies without text.
pub fn message_preview(html: &str, attachments: &[AttachmentInfo]) -> Option<String> {
    if html.contains("<video") {
        return Some(VIDEO_PREVIEW.to_owned());
    }
    let mut parts = BodyParts::default();
    collect(&html_to_spans(html), &mut parts);
    if let Some(text) = collapse(&parts.text) {
        return Some(text);
    }
    if parts.gifs > 0 {
        return Some(GIF_PREVIEW.to_owned());
    }
    if parts.images > 0 {
        return Some(IMAGE_PREVIEW.to_owned());
    }
    attachment_label(attachments).map(str::to_owned)
}

fn attachment_label(attachments: &[AttachmentInfo]) -> Option<&'static str> {
    let has_type = |wanted: &str| {
        attachments
            .iter()
            .any(|attachment| attachment.content_type.as_deref() == Some(wanted))
    };
    if has_type(FORWARDED_CONTENT_TYPE) {
        Some(FORWARD_PREVIEW)
    } else if has_type(FILE_CONTENT_TYPE) {
        Some(FILE_PREVIEW)
    } else if has_type(AUDIO_CARD_TYPE) {
        Some(AUDIO_PREVIEW)
    } else if has_type(LOOP_CARD_TYPE) {
        Some(LOOP_PREVIEW)
    } else if attachments.iter().any(AttachmentInfo::is_card) {
        Some(CARD_PREVIEW)
    } else {
        None
    }
}

fn collect(spans: &[Span], parts: &mut BodyParts) {
    for span in spans {
        match span {
            Span::Text(value) | Span::Code(value) => parts.text.push_str(value),
            Span::CodeBlock { code, .. } => {
                parts.text.push(' ');
                parts.text.push_str(code);
                parts.text.push(' ');
            }
            Span::Mention { name, .. } => parts.text.push_str(name),
            Span::Quote { .. } => {}
            Span::Image { hosted_content_url } if is_giphy_url(hosted_content_url) => {
                parts.gifs += 1
            }
            Span::Image { .. } => parts.images += 1,
            Span::LineBreak | Span::Rule => parts.text.push(' '),
            Span::Bold(children)
            | Span::Italic(children)
            | Span::Strike(children)
            | Span::Superscript(children)
            | Span::Subscript(children)
            | Span::Sized(_, children)
            | Span::Underline(children)
            | Span::Colored { children, .. }
            | Span::Link { children, .. } => collect(children, parts),
            Span::Heading { children, .. } | Span::BlockQuote(children) => {
                parts.text.push(' ');
                collect(children, parts);
                parts.text.push(' ');
            }
            Span::List { items, .. } => {
                for item in items {
                    parts.text.push(' ');
                    collect(item, parts);
                }
                parts.text.push(' ');
            }
            Span::Table { rows, .. } => {
                for cell in rows.iter().flatten() {
                    parts.text.push(' ');
                    collect(cell, parts);
                }
                parts.text.push(' ');
            }
        }
    }
}

fn collapse(text: &str) -> Option<String> {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }
    Some(collapsed.chars().take(MAX_PREVIEW_CHARS).collect())
}

fn system_event_preview(detail: &Value) -> Option<&'static str> {
    match detail.get("@odata.type")?.as_str()? {
        RECORDING_TYPE => match detail.get("callRecordingStatus")?.as_str()? {
            "initial" => Some(RECORDING_STARTED_PREVIEW),
            "chunkFinished" => Some(RECORDING_STOPPED_PREVIEW),
            "success" => Some(RECORDING_READY_PREVIEW),
            _ => None,
        },
        TRANSCRIPT_TYPE => Some(TRANSCRIPT_PREVIEW),
        MEMBERS_JOINED_TYPE => Some(USER_JOINED_PREVIEW),
        MEMBERS_ADDED_TYPE if added_only_self(detail) => Some(USER_JOINED_PREVIEW),
        MEMBERS_ADDED_TYPE => Some(USER_ADDED_PREVIEW),
        _ => None,
    }
}

fn call_phase_preview(phase: CallPhase) -> Option<&'static str> {
    match phase {
        CallPhase::RecordingStarted => Some(RECORDING_STARTED_PREVIEW),
        CallPhase::RecordingStopped => Some(RECORDING_STOPPED_PREVIEW),
        CallPhase::RecordingSaved => Some(RECORDING_READY_PREVIEW),
        CallPhase::TranscriptStarted => Some(TRANSCRIPT_PREVIEW),
        CallPhase::Started | CallPhase::Ended => None,
    }
}

/// Teams leaves call start and end out of the chat list line; recordings and transcripts stay in it.
pub(crate) fn keeps_previous_line(record: &MessageRecord) -> bool {
    call_event(record).is_some_and(|event| event.phase.is_call())
}

fn added_only_self(detail: &Value) -> bool {
    let Some(initiator) = detail.pointer("/initiator/user/id").and_then(Value::as_str) else {
        return false;
    };
    let members = detail
        .get("members")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    !members.is_empty()
        && members
            .iter()
            .all(|member| member.get("id").and_then(Value::as_str) == Some(initiator))
}

pub(crate) struct OwnedPreview {
    pub text: Option<String>,
    pub sender_id: Option<String>,
    pub sender_name: Option<String>,
    pub deleted: bool,
    pub system: bool,
    pub call: bool,
}

impl OwnedPreview {
    pub(crate) fn from_graph(message: &Message) -> Self {
        let regular = message
            .message_type
            .as_deref()
            .is_none_or(|kind| kind == "message");
        let deleted = message.is_deleted();
        let detail = message.event_detail.as_ref().filter(|_| !regular);
        let call = detail
            .and_then(CallEvent::from_graph)
            .is_some_and(|event| event.phase.is_call());
        let text = if deleted {
            None
        } else if regular {
            message
                .body
                .as_ref()
                .and_then(|body| body.content.as_deref())
                .and_then(|html| message_preview(html, &[]))
        } else {
            detail.and_then(system_event_preview).map(str::to_owned)
        };
        let sender = message.from.as_ref().filter(|_| regular);
        OwnedPreview {
            text,
            sender_id: sender
                .and_then(|sender| sender.user_id())
                .map(str::to_owned),
            sender_name: sender
                .and_then(|sender| sender.display_name())
                .map(str::to_owned),
            deleted,
            system: !regular,
            call,
        }
    }

    pub(crate) fn from_record(record: &MessageRecord) -> Self {
        let event = call_event(record);
        let text = if record.deleted {
            None
        } else if let Some(event) = &event {
            call_phase_preview(event.phase).map(str::to_owned)
        } else {
            message_preview(&record.body_html, &attachments(record))
        };
        let sender = event.is_none();
        OwnedPreview {
            text,
            sender_id: record.sender_id.clone().filter(|_| sender),
            sender_name: record.sender_name.clone().filter(|_| sender),
            deleted: record.deleted,
            system: event.is_some(),
            call: event.is_some_and(|event| event.phase.is_call()),
        }
    }

    pub(crate) fn as_store(&self) -> ChatPreview<'_> {
        ChatPreview {
            text: self.text.as_deref(),
            sender_id: self.sender_id.as_deref(),
            sender_name: self.sender_name.as_deref(),
            deleted: self.deleted,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attachment(content_type: &str) -> AttachmentInfo {
        serde_json::from_value(serde_json::json!({
            "content_type": content_type,
            "name": null,
            "url": null,
            "text": null,
        }))
        .unwrap()
    }

    fn graph_message(value: serde_json::Value) -> Message {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn strips_tags_and_collapses_whitespace() {
        assert_eq!(
            preview_text("<p>Hello <b>big</b>\n  world</p><p>second</p>").as_deref(),
            Some("Hello big world second")
        );
    }

    #[test]
    fn image_only_bodies_are_empty() {
        assert_eq!(preview_text("<p><img src=\"x\"></p>"), None);
        assert_eq!(preview_text("   "), None);
    }

    #[test]
    fn long_text_is_cut_to_the_limit() {
        let long = "a ".repeat(400);
        assert_eq!(
            preview_text(&long).unwrap().chars().count(),
            MAX_PREVIEW_CHARS
        );
    }

    #[test]
    fn plain_text_bodies_keep_entities_decoded() {
        assert_eq!(preview_text("a &amp; b").as_deref(), Some("a & b"));
    }

    #[test]
    fn emoji_and_mentions_read_like_teams() {
        let html = r#"<p><at id="0">Robin Beck</at>&nbsp;kannst du?&nbsp;<emoji id="smile" alt="🙂" title="Smile"></emoji></p>"#;
        assert_eq!(
            preview_text(html).as_deref(),
            Some("Robin Beck kannst du? 🙂")
        );
    }

    #[test]
    fn reply_quotes_are_left_out() {
        let html = r#"<blockquote itemtype="http://schema.skype.com/Reply" itemid="1"><strong>Ada</strong><p>old</p></blockquote><p>new</p>"#;
        assert_eq!(preview_text(html).as_deref(), Some("new"));
    }

    #[test]
    fn image_only_message_says_sent_an_image() {
        let html = r#"<p><img src="https://graph.microsoft.com/v1.0/chats/19:x/messages/1/hostedContents/aWQ9/$value" alt="image" width="447" height="250" itemid="0-nde-d3-x"></p>"#;
        assert_eq!(message_preview(html, &[]).as_deref(), Some(IMAGE_PREVIEW));
    }

    #[test]
    fn text_with_an_image_keeps_only_the_text() {
        let html = r#"<p>this is from QA:</p><p><img src="https://graph.microsoft.com/v1.0/x/$value"></p><p>&nbsp;</p>"#;
        assert_eq!(
            message_preview(html, &[]).as_deref(),
            Some("this is from QA:")
        );
    }

    #[test]
    fn giphy_reads_as_gif() {
        let html = r#"<p>&nbsp;</p><img alt="Happy Birthday Reaction GIF by The Office (GIF Image)" height="250" width="300" src="https://media2.giphy.com/media/g5R9dok94mrIvplmZd/giphy.gif?cid=x&amp;ct=g"><p>&nbsp;</p>"#;
        assert_eq!(message_preview(html, &[]).as_deref(), Some(GIF_PREVIEW));
    }

    #[test]
    fn inline_video_reads_as_video() {
        let html = r#"<p>look</p><video src="https://x/v.mp4"></video>"#;
        assert_eq!(message_preview(html, &[]).as_deref(), Some(VIDEO_PREVIEW));
    }

    #[test]
    fn attachment_only_bodies_get_a_label() {
        let body = r#"<attachment id="1792149081221"></attachment>"#;
        let cases = [
            (FORWARDED_CONTENT_TYPE, FORWARD_PREVIEW),
            ("reference", FILE_PREVIEW),
            ("application/vnd.microsoft.card.adaptive", CARD_PREVIEW),
            ("application/vnd.microsoft.card.hero", CARD_PREVIEW),
            (AUDIO_CARD_TYPE, AUDIO_PREVIEW),
            (LOOP_CARD_TYPE, LOOP_PREVIEW),
        ];
        for (content_type, expected) in cases {
            assert_eq!(
                message_preview(body, &[attachment(content_type)]).as_deref(),
                Some(expected),
                "{content_type}"
            );
        }
        assert_eq!(message_preview(body, &[]), None);
    }

    #[test]
    fn a_file_with_text_shows_the_text() {
        let body = r#"Hi Philipp,<br>Kann's dir zeigen.<attachment id="0cf57a46"></attachment>"#;
        assert_eq!(
            message_preview(body, &[attachment("reference")]).as_deref(),
            Some("Hi Philipp, Kann's dir zeigen.")
        );
    }

    #[test]
    fn a_reply_shows_the_reply_text() {
        let body = "<attachment id=\"1792136042729\"></attachment>\nwas war mit dependabot?";
        assert_eq!(
            message_preview(body, &[attachment("messageReference")]).as_deref(),
            Some("was war mit dependabot?")
        );
    }

    #[test]
    fn graph_recording_and_transcript_events_have_teams_text() {
        let event = |detail: serde_json::Value| {
            OwnedPreview::from_graph(&graph_message(serde_json::json!({
                "id": "1",
                "messageType": "systemEventMessage",
                "body": {"contentType": "html", "content": "<systemEventMessage/>"},
                "from": {"user": {"id": "u1", "displayName": "Ada Lovelace"}},
                "eventDetail": detail,
            })))
        };
        let ready = event(serde_json::json!({
            "@odata.type": RECORDING_TYPE,
            "callId": "a64b5e71",
            "callRecordingDisplayName": "Call with Echo-20261015_120906-Meeting Recording.mp4",
            "callRecordingStatus": "success",
        }));
        assert_eq!(ready.text.as_deref(), Some(RECORDING_READY_PREVIEW));
        assert!(ready.system && !ready.call);
        assert_eq!(ready.sender_name, None);
        let started = event(serde_json::json!({
            "@odata.type": RECORDING_TYPE,
            "callRecordingStatus": "initial",
        }));
        assert_eq!(started.text.as_deref(), Some(RECORDING_STARTED_PREVIEW));
        let transcript = event(serde_json::json!({"@odata.type": TRANSCRIPT_TYPE}));
        assert_eq!(transcript.text.as_deref(), Some(TRANSCRIPT_PREVIEW));
    }

    #[test]
    fn graph_member_events_say_added_or_joined() {
        let added = |members: serde_json::Value| {
            OwnedPreview::from_graph(&graph_message(serde_json::json!({
                "id": "1",
                "messageType": "systemEventMessage",
                "eventDetail": {
                    "@odata.type": MEMBERS_ADDED_TYPE,
                    "members": members,
                    "initiator": {"user": {"id": "me"}},
                },
            })))
            .text
        };
        assert_eq!(
            added(serde_json::json!([{"id": "other"}])).as_deref(),
            Some(USER_ADDED_PREVIEW)
        );
        assert_eq!(
            added(serde_json::json!([{"id": "me"}])).as_deref(),
            Some(USER_JOINED_PREVIEW)
        );
    }

    #[test]
    fn graph_call_events_are_marked_to_keep_the_previous_line() {
        let preview = OwnedPreview::from_graph(&graph_message(serde_json::json!({
            "id": "1",
            "messageType": "systemEventMessage",
            "eventDetail": {
                "@odata.type": "#microsoft.graph.callEndedEventMessageDetail",
                "callId": "c1",
                "callDuration": "PT30S",
                "callEventType": "call",
            },
        })));
        assert!(preview.call);
        assert_eq!(preview.text, None);
    }

    #[test]
    fn graph_deleted_flag_marks_the_preview_deleted() {
        let preview = OwnedPreview::from_graph(&graph_message(serde_json::json!({
            "id": "1",
            "messageType": "message",
            "isDeleted": true,
            "body": {"contentType": "html", "content": "secret"},
        })));
        assert!(preview.deleted);
        assert_eq!(preview.text, None);
    }

    #[test]
    fn graph_image_preview_says_sent_an_image() {
        let preview = OwnedPreview::from_graph(&graph_message(serde_json::json!({
            "id": "1",
            "messageType": "message",
            "body": {"contentType": "html", "content": "<p><img src=\"https://graph.microsoft.com/x/$value\"></p>"},
            "from": {"user": {"id": "u1", "displayName": "Lukas Berg"}},
        })));
        assert_eq!(preview.text.as_deref(), Some(IMAGE_PREVIEW));
        assert_eq!(preview.sender_name.as_deref(), Some("Lukas Berg"));
    }

    fn call_record(phase: &str) -> MessageRecord {
        MessageRecord {
            conversation_id: "19:chat".into(),
            message_id: "1".into(),
            sender_id: Some("u1".into()),
            sender_name: Some("Ada Lovelace".into()),
            call_event_json: Some(
                serde_json::json!({
                    "phase": phase,
                    "call_type": "Call",
                    "call_id": "c1",
                    "duration_seconds": null,
                    "initiator_id": null,
                    "initiator_name": null,
                    "participant_ids": [],
                })
                .to_string(),
            ),
            ..Default::default()
        }
    }

    #[test]
    fn stored_recordings_fill_the_line_and_calls_keep_the_previous_one() {
        let saved = call_record("RecordingSaved");
        assert!(!keeps_previous_line(&saved));
        let preview = OwnedPreview::from_record(&saved);
        assert_eq!(preview.text.as_deref(), Some(RECORDING_READY_PREVIEW));
        assert_eq!(preview.sender_name, None);
        assert!(!preview.call);
        let transcript = OwnedPreview::from_record(&call_record("TranscriptStarted"));
        assert_eq!(transcript.text.as_deref(), Some(TRANSCRIPT_PREVIEW));
        let ended = call_record("Ended");
        assert!(keeps_previous_line(&ended));
        assert!(OwnedPreview::from_record(&ended).call);
    }
}
