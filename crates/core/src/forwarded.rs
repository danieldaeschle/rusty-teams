use chrono::{DateTime, Utc};
use serde::Deserialize;
use store::MessageRecord;

use crate::markdown::escape_html;
use crate::spans::html_to_spans;
use crate::stored::{AttachmentInfo, attachments, push_plain};

pub const FORWARDED_CONTENT_TYPE: &str = "forwardedMessageReference";
pub(crate) const FORWARD_ITEMTYPE: &str = "schema.skype.com/Forward";

/// The original message carried by a Graph `forwardedMessageReference` attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardInfo {
    pub attachment_id: Option<String>,
    pub message_id: String,
    pub conversation_id: Option<String>,
    pub sender_id: Option<String>,
    pub sender_name: Option<String>,
    pub sent_at: Option<DateTime<Utc>>,
    pub content_html: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ForwardContent {
    original_message_id: Option<String>,
    #[serde(default)]
    original_message_content: String,
    original_conversation_id: Option<String>,
    original_sent_date_time: Option<DateTime<Utc>>,
    original_message_sender: Option<graph::Sender>,
}

impl ForwardInfo {
    pub fn from_attachment(attachment: &AttachmentInfo) -> Option<ForwardInfo> {
        if attachment.content_type.as_deref() != Some(FORWARDED_CONTENT_TYPE) {
            return None;
        }
        let content: ForwardContent = serde_json::from_str(attachment.content.as_deref()?).ok()?;
        let sender = content.original_message_sender.as_ref();
        Some(ForwardInfo {
            message_id: content
                .original_message_id
                .or_else(|| attachment.id.clone())?,
            attachment_id: attachment.id.clone(),
            conversation_id: content.original_conversation_id,
            sender_id: sender
                .and_then(|sender| sender.user_id())
                .map(str::to_owned),
            sender_name: sender
                .and_then(|sender| sender.display_name())
                .map(str::to_owned),
            sent_at: content.original_sent_date_time,
            content_html: content.original_message_content,
        })
    }

    pub fn text(&self) -> String {
        let mut text = String::new();
        push_plain(&mut text, &html_to_spans(&self.content_html));
        text.trim().to_owned()
    }

    pub fn html(&self, sender_name: Option<&str>) -> String {
        let sender = sender_name
            .or(self.sender_name.as_deref())
            .map(|name| format!("<strong>{}</strong>", escape_html(name)))
            .unwrap_or_default();
        let time = self
            .sent_at
            .map(|time| {
                let local = time.with_timezone(&chrono::Local);
                format!(" <span>{}</span>", local.format("%d/%m/%Y %H:%M"))
            })
            .unwrap_or_default();
        format!(
            "<blockquote itemscope=\"\" itemtype=\"http://{FORWARD_ITEMTYPE}\" itemid=\"{}\">{sender}{time}{}</blockquote>",
            escape_html(&self.message_id),
            self.content_html,
        )
    }
}

pub fn forwards(record: &MessageRecord) -> Vec<ForwardInfo> {
    if record.deleted {
        return Vec::new();
    }
    attachments(record)
        .iter()
        .filter_map(ForwardInfo::from_attachment)
        .collect()
}

pub fn forwarded_texts(record: &MessageRecord) -> Vec<String> {
    forwards(record)
        .iter()
        .map(ForwardInfo::text)
        .filter(|text| !text.is_empty())
        .collect()
}

pub fn is_forwarded(record: &MessageRecord) -> bool {
    !record.deleted && (record.body_html.contains(FORWARD_ITEMTYPE) || !forwards(record).is_empty())
}

/// `html` with each forward's `<attachment id>` tag replaced by its forward blockquote; a forward without a tag goes last.
pub(crate) fn inline_forwards(
    record: &MessageRecord,
    mut html: String,
    sender_name: &dyn Fn(&str) -> Option<String>,
) -> String {
    for forward in forwards(record) {
        let name = forward.sender_id.as_deref().and_then(sender_name);
        let block = forward.html(name.as_deref());
        let tag = forward
            .attachment_id
            .as_deref()
            .map(|id| format!("<attachment id=\"{id}\"></attachment>"))
            .filter(|tag| html.contains(tag));
        match tag {
            Some(tag) => html = html.replacen(&tag, &block, 1),
            None => html.push_str(&block),
        }
    }
    html
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mapping::message_record;
    use crate::spans::Span;
    use crate::stored::{copy_text, edit_preserved, linked_message_spans_with, readable_text};

    const CHAT_ID: &str = "19:11111111-1111-4111-8111-111111111111_22222222-2222-4222-8222-222222222222@unq.gbl.spaces";
    const ORIGINAL_SENDER_ID: &str = "33333333-3333-4333-8333-333333333333";
    const ORIGINAL_TEXT: &str = "Keep it short and make sure everyone gets the key part";

    fn forwarded_record() -> MessageRecord {
        let message: graph::Message =
            serde_json::from_str(include_str!("../tests/fixtures/forwarded_message.json")).unwrap();
        message_record(CHAT_ID, &message).unwrap()
    }

    fn plain(spans: &[Span]) -> String {
        let mut text = String::new();
        push_plain(&mut text, spans);
        text
    }

    #[test]
    fn a_graph_forward_keeps_the_original_message() {
        let record = forwarded_record();
        assert!(is_forwarded(&record));
        let found = forwards(&record);
        assert_eq!(found.len(), 1);
        let forward = &found[0];
        assert_eq!(forward.message_id, "1792149081221");
        assert_eq!(forward.attachment_id.as_deref(), Some("1792149081221"));
        assert_eq!(forward.sender_id.as_deref(), Some(ORIGINAL_SENDER_ID));
        assert_eq!(forward.sender_name, None);
        assert_eq!(
            forward.conversation_id.as_deref(),
            Some("19:5555555555554555a5555555555555aa@thread.v2")
        );
        assert_eq!(
            forward.sent_at,
            Some("2026-10-16T11:11:21.221Z".parse().unwrap())
        );
        assert!(forward.text().starts_with(ORIGINAL_TEXT));
    }

    #[test]
    fn the_forward_replaces_its_attachment_tag_with_a_quote_naming_the_original_sender() {
        let record = forwarded_record();
        let spans = linked_message_spans_with(&record, &|_| None, &|user_id| {
            (user_id == ORIGINAL_SENDER_ID).then(|| "Ada Original".to_owned())
        });
        let [Span::BlockQuote(children)] = spans.as_slice() else {
            panic!("expected one forward blockquote, got {spans:?}");
        };
        let text = plain(children);
        assert!(text.starts_with("Ada Original"), "{text}");
        assert!(text.contains(ORIGINAL_TEXT), "{text}");
    }

    #[test]
    fn the_forwarded_text_is_copied_but_not_part_of_the_own_text() {
        let record = forwarded_record();
        assert_eq!(readable_text(&record), "");
        assert!(copy_text(&record).starts_with(ORIGINAL_TEXT));
        assert_eq!(forwarded_texts(&record).len(), 1);
    }

    #[test]
    fn an_edit_keeps_the_forward_attachment() {
        let preserved = edit_preserved(&forwarded_record());
        assert_eq!(preserved.kept.len(), 1);
        assert_eq!(
            preserved.kept[0].content_type.as_deref(),
            Some(FORWARDED_CONTENT_TYPE)
        );
    }

    #[test]
    fn a_deleted_forward_is_not_forwarded() {
        let mut record = forwarded_record();
        record.deleted = true;
        assert!(!is_forwarded(&record));
        assert!(forwards(&record).is_empty());
    }
}
