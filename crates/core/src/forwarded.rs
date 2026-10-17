use std::sync::LazyLock;

use chrono::{DateTime, Utc};
use regex::Regex;
use serde::Deserialize;
use store::MessageRecord;

use crate::markdown::escape_html;
use crate::spans::html_to_spans;
use crate::stored::{AttachmentInfo, attachments, push_plain};

pub const FORWARDED_CONTENT_TYPE: &str = "forwardedMessageReference";
pub(crate) const FORWARD_ITEMTYPE: &str = "schema.skype.com/Forward";
pub const FORWARD_LOADING_TEXT: &str = "Loading forwarded message";
pub const FORWARD_UNAVAILABLE_TEXT: &str = "Original message not available";

static MENTION_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<at\b[^>]*>").expect("mention tag pattern"));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardState {
    Ready,
    /// Cached without its original; the heal fetches it.
    Loading,
    Unavailable,
}

/// The original message carried by a Graph `forwardedMessageReference` attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardInfo {
    pub state: ForwardState,
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
        let Some(raw) = attachment.content.as_deref() else {
            return Some(ForwardInfo::without_content(attachment));
        };
        let content: ForwardContent = serde_json::from_str(raw).ok()?;
        let sender = content.original_message_sender.as_ref();
        Some(ForwardInfo {
            state: ForwardState::Ready,
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

    fn without_content(attachment: &AttachmentInfo) -> ForwardInfo {
        ForwardInfo {
            state: if attachment.unavailable {
                ForwardState::Unavailable
            } else {
                ForwardState::Loading
            },
            attachment_id: attachment.id.clone(),
            message_id: attachment.id.clone().unwrap_or_default(),
            conversation_id: None,
            sender_id: None,
            sender_name: None,
            sent_at: None,
            content_html: String::new(),
        }
    }

    pub fn text(&self) -> String {
        let mut text = String::new();
        push_plain(&mut text, &html_to_spans(&self.content_html));
        text.trim().to_owned()
    }

    pub fn html(&self, header: &ForwardHeader) -> String {
        let sender = header
            .sender
            .as_deref()
            .or(self.sender_name.as_deref())
            .map(|name| format!("<strong>{}</strong>", escape_html(name)))
            .unwrap_or_default();
        let time = header
            .time
            .as_deref()
            .map(|time| format!(" <span>{}</span>", escape_html(time)))
            .unwrap_or_default();
        let content = match self.state {
            ForwardState::Ready => MENTION_ID.replace_all(&self.content_html, "<at>"),
            ForwardState::Loading => format!("<p><em>{FORWARD_LOADING_TEXT}</em></p>").into(),
            ForwardState::Unavailable => {
                format!("<p><em>{FORWARD_UNAVAILABLE_TEXT}</em></p>").into()
            }
        };
        format!(
            "<blockquote itemscope=\"\" itemtype=\"http://{FORWARD_ITEMTYPE}\" itemid=\"{}\">{sender}{time}{content}</blockquote>",
            escape_html(&self.message_id),
        )
    }
}

/// What the forward quote names above the original: sender display name and sent time label.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ForwardHeader {
    pub sender: Option<String>,
    pub time: Option<String>,
}

pub fn forwards(record: &MessageRecord) -> Vec<ForwardInfo> {
    if record.deleted || !record.attachments_json.contains(FORWARDED_CONTENT_TYPE) {
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

fn is_stale_forward(attachment: &AttachmentInfo) -> bool {
    attachment.content_type.as_deref() == Some(FORWARDED_CONTENT_TYPE)
        && attachment.content.is_none()
        && !attachment.unavailable
}

/// A forward attachment cached before its content was kept, and not given up on.
pub(crate) fn lacks_forward_content(record: &MessageRecord) -> bool {
    attachments(record).iter().any(is_stale_forward)
}

/// `record` with each stale forward marked as unavailable, so it is not fetched again.
pub(crate) fn mark_forwards_unavailable(record: &MessageRecord) -> Option<MessageRecord> {
    let mut stored = attachments(record);
    let mut changed = false;
    for attachment in stored.iter_mut().filter(|attachment| is_stale_forward(attachment)) {
        attachment.unavailable = true;
        changed = true;
    }
    let attachments_json = serde_json::to_string(&stored).ok()?;
    changed.then(|| MessageRecord {
        attachments_json,
        ..record.clone()
    })
}

/// `html` with each forward's `<attachment id>` tag replaced by its forward blockquote; a forward without a tag goes last.
pub(crate) fn inline_forwards(
    record: &MessageRecord,
    mut html: String,
    header: &dyn Fn(&ForwardInfo) -> ForwardHeader,
) -> String {
    for forward in forwards(record) {
        let block = forward.html(&header(&forward));
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

pub fn no_forward_header(_: &ForwardInfo) -> ForwardHeader {
    ForwardHeader::default()
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
        let spans = linked_message_spans_with(&record, &|_| None, &|forward| ForwardHeader {
            sender: (forward.sender_id.as_deref() == Some(ORIGINAL_SENDER_ID))
                .then(|| "Ada Original".to_owned()),
            time: Some("13:11".to_owned()),
        });
        let [Span::BlockQuote(children)] = spans.as_slice() else {
            panic!("expected one forward blockquote, got {spans:?}");
        };
        let text = plain(children);
        assert!(text.starts_with("Ada Original 13:11"), "{text}");
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
    fn a_mention_inside_the_forward_does_not_borrow_the_forwarders_mention() {
        let mut record = forwarded_record();
        record.body_html = format!(
            r#"<p><at id="0">Bob Sample</at></p>{}"#,
            record.body_html
        );
        record.attachments_json = record
            .attachments_json
            .replace("<p>Keep", r#"<p><at id=\\\"0\\\">Cleo Other</at> Keep"#);
        record.mentions_json =
            r#"[{"user_id":"user-bob","name":"Bob Sample","id":0}]"#.to_owned();
        assert!(forwards(&record)[0].content_html.contains("Cleo Other"));
        let spans = linked_message_spans_with(&record, &|_| None, &no_forward_header);
        let mut mentions = Vec::new();
        collect_mentions(&spans, &mut mentions);
        assert_eq!(
            mentions,
            [
                ("Bob Sample".to_owned(), Some("teams-profile:user-bob".to_owned())),
                ("Cleo Other".to_owned(), None),
            ]
        );
    }

    fn collect_mentions(spans: &[Span], found: &mut Vec<(String, Option<String>)>) {
        for span in spans {
            match span {
                Span::Mention { name, id } => found.push((name.clone(), id.clone())),
                Span::BlockQuote(children) | Span::Bold(children) => {
                    collect_mentions(children, found);
                }
                _ => {}
            }
        }
    }

    #[test]
    fn a_forward_cached_without_its_content_needs_a_refetch() {
        let mut record = forwarded_record();
        assert!(!lacks_forward_content(&record));
        record.attachments_json = r#"[{"content_type":"forwardedMessageReference","name":null,"url":null,"text":null,"size":null,"quote":null,"id":"1792149081221","content":null}]"#.to_owned();
        assert!(lacks_forward_content(&record));
        let found = forwards(&record);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].state, ForwardState::Loading);
        assert!(forwarded_texts(&record).is_empty());
    }

    fn quote_text(record: &MessageRecord) -> String {
        let spans = linked_message_spans_with(record, &|_| None, &no_forward_header);
        let [Span::BlockQuote(children)] = spans.as_slice() else {
            panic!("expected one forward blockquote, got {spans:?}");
        };
        plain(children)
    }

    #[test]
    fn a_forward_without_content_shows_a_loading_line_until_it_is_given_up() {
        let mut record = forwarded_record();
        record.attachments_json = r#"[{"content_type":"forwardedMessageReference","name":null,"url":null,"text":null,"size":null,"quote":null,"id":"1792149081221","content":null}]"#.to_owned();
        assert!(is_forwarded(&record));
        assert_eq!(quote_text(&record).trim(), FORWARD_LOADING_TEXT);

        let given_up = mark_forwards_unavailable(&record).unwrap();
        assert!(!lacks_forward_content(&given_up));
        assert_eq!(forwards(&given_up)[0].state, ForwardState::Unavailable);
        assert_eq!(quote_text(&given_up).trim(), FORWARD_UNAVAILABLE_TEXT);
        assert!(mark_forwards_unavailable(&given_up).is_none());
        assert!(edit_preserved(&given_up).kept.is_empty());
    }

    #[test]
    fn a_deleted_forward_is_not_forwarded() {
        let mut record = forwarded_record();
        record.deleted = true;
        assert!(!is_forwarded(&record));
        assert!(forwards(&record).is_empty());
    }
}
