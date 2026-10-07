use chrono::{DateTime, Utc};
use scraper::{ElementRef, Html};
use serde::{Deserialize, Serialize};
use store::MessageRecord;

use crate::spans::{Span, html_to_spans};

const EMOJI_ITEMTYPE: &str = "schema.skype.com/Emoji";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentInfo {
    pub content_type: Option<String>,
    pub name: Option<String>,
    pub url: Option<String>,
    pub text: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub quote: Option<QuoteInfo>,
    #[serde(default)]
    pub id: Option<String>,
    /// Raw content, kept only for quotes and cards so an edit can send them back unchanged.
    #[serde(default)]
    pub content: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Word,
    Excel,
    PowerPoint,
    Pdf,
    Image,
    Archive,
    Other,
}

impl FileKind {
    pub fn from_name(name: &str) -> FileKind {
        let Some((_, extension)) = name.rsplit_once('.') else {
            return FileKind::Other;
        };
        match extension.to_ascii_lowercase().as_str() {
            "doc" | "docx" | "docm" | "dot" | "dotx" | "rtf" | "odt" => FileKind::Word,
            "xls" | "xlsx" | "xlsm" | "xlsb" | "csv" | "ods" => FileKind::Excel,
            "ppt" | "pptx" | "pptm" | "pps" | "ppsx" | "odp" => FileKind::PowerPoint,
            "pdf" => FileKind::Pdf,
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg" | "heic" | "tif" | "tiff" => {
                FileKind::Image
            }
            "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" => FileKind::Archive,
            _ => FileKind::Other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuoteInfo {
    pub message_id: String,
    pub preview: String,
    pub sender_name: Option<String>,
}

/// Image inside a message body. `key()` is the image cache key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRef {
    pub id: String,
    pub url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

impl ImageRef {
    pub fn key(&self) -> &str {
        &self.url
    }
}

impl AttachmentInfo {
    pub fn is_card(&self) -> bool {
        self.content_type
            .as_deref()
            .is_some_and(|content_type| content_type.contains("card"))
    }

    pub fn file_kind(&self) -> FileKind {
        match self.name.as_deref().map(FileKind::from_name) {
            Some(FileKind::Other) | None if self.has_image_content_type() => FileKind::Image,
            Some(kind) => kind,
            None => FileKind::Other,
        }
    }

    fn has_image_content_type(&self) -> bool {
        self.content_type
            .as_deref()
            .is_some_and(|content_type| content_type.starts_with("image/"))
    }

    pub fn is_file(&self) -> bool {
        self.content_type.as_deref() == Some("reference") && self.url.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReactionInfo {
    pub reaction_type: String,
    pub user_id: Option<String>,
    pub user_name: Option<String>,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MentionInfo {
    pub user_id: Option<String>,
    pub name: String,
}

pub fn attachments(record: &MessageRecord) -> Vec<AttachmentInfo> {
    serde_json::from_str(&record.attachments_json).unwrap_or_default()
}

pub fn reactions(record: &MessageRecord) -> Vec<ReactionInfo> {
    serde_json::from_str(&record.reactions_json).unwrap_or_default()
}

pub fn mentions(record: &MessageRecord) -> Vec<MentionInfo> {
    serde_json::from_str(&record.mentions_json).unwrap_or_default()
}

pub fn quotes(record: &MessageRecord) -> Vec<QuoteInfo> {
    attachments(record)
        .into_iter()
        .filter_map(|attachment| attachment.quote)
        .collect()
}

pub fn images(record: &MessageRecord) -> Vec<ImageRef> {
    if record.deleted {
        return Vec::new();
    }
    let fragment = Html::parse_fragment(&record.body_html);
    let mut found: Vec<ImageRef> = Vec::new();
    for element in fragment
        .root_element()
        .descendants()
        .filter_map(ElementRef::wrap)
    {
        let value = element.value();
        let is_emoji = value
            .attr("itemtype")
            .is_some_and(|itemtype| itemtype.contains(EMOJI_ITEMTYPE));
        let Some(url) = value
            .attr("src")
            .filter(|url| value.name() == "img" && !is_emoji && !url.is_empty())
        else {
            continue;
        };
        if found.iter().any(|image| image.url == url) {
            continue;
        }
        let style = value.attr("style").unwrap_or_default();
        found.push(ImageRef {
            id: image_id(url),
            url: url.to_owned(),
            width: dimension(value.attr("width"), style, "width"),
            height: dimension(value.attr("height"), style, "height"),
        });
    }
    found
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditPreserved {
    pub images_html: String,
    pub files: Vec<graph::FileReference>,
    pub kept: Vec<graph::KeptAttachment>,
}

/// What an edit must carry over: the `<img>` elements, the file attachments and the quote or card attachments, each matched to its `<attachment id>` tag.
pub fn edit_preserved(record: &MessageRecord) -> EditPreserved {
    let fragment = Html::parse_fragment(&record.body_html);
    let mut preserved = EditPreserved::default();
    let mut tag_ids: Vec<&str> = Vec::new();
    for element in fragment
        .root_element()
        .descendants()
        .filter_map(ElementRef::wrap)
    {
        let value = element.value();
        let is_emoji = value
            .attr("itemtype")
            .is_some_and(|itemtype| itemtype.contains(EMOJI_ITEMTYPE));
        match value.name() {
            "img" if !is_emoji && value.attr("src").is_some_and(|src| !src.is_empty()) => {
                preserved
                    .images_html
                    .push_str(&format!("<p>{}</p>", element.html()));
            }
            "attachment" => tag_ids.extend(value.attr("id")),
            _ => {}
        }
    }
    let stored = attachments(record);
    let attachment_id = |attachment: &AttachmentInfo| {
        attachment.id.clone().or_else(|| {
            attachment
                .quote
                .as_ref()
                .map(|quote| quote.message_id.clone())
        })
    };
    let mut unclaimed: Vec<&str> = tag_ids
        .iter()
        .copied()
        .filter(|id| {
            !stored.iter().any(|attachment| {
                !attachment.is_file() && attachment_id(attachment).as_deref() == Some(*id)
            })
        })
        .collect();
    for attachment in stored {
        if attachment.is_file() {
            let id = match attachment_id(&attachment) {
                Some(id) if tag_ids.contains(&id.as_str()) => id,
                Some(_) => continue,
                None if unclaimed.is_empty() => continue,
                None => unclaimed.remove(0).to_owned(),
            };
            if let Some(content_url) = attachment.url {
                preserved.files.push(graph::FileReference {
                    attachment_id: id,
                    content_url,
                    name: attachment.name.unwrap_or_default(),
                });
            }
        } else if let Some(id) = attachment_id(&attachment)
            && tag_ids.contains(&id.as_str())
            && attachment.content.is_some()
        {
            preserved.kept.push(graph::KeptAttachment {
                id,
                content_type: attachment.content_type,
                content: attachment.content,
                content_url: attachment.url,
                name: attachment.name,
            });
        }
    }
    preserved
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileCard {
    pub name: String,
    pub kind: FileKind,
    pub content_type: Option<String>,
    pub size: Option<u64>,
    pub open_url: String,
}

/// File attachments (`reference` type) with a name and a URL. `size` is rarely known; Graph does not send it for these.
pub fn files(record: &MessageRecord) -> Vec<FileCard> {
    attachments(record)
        .into_iter()
        .filter(AttachmentInfo::is_file)
        .map(|attachment| FileCard {
            kind: attachment.file_kind(),
            name: attachment.name.clone().unwrap_or_default(),
            content_type: attachment.content_type.clone(),
            size: attachment.size,
            open_url: attachment.url.clone().unwrap_or_default(),
        })
        .collect()
}

/// Text for the clipboard: body and card text, mentions as `@name`, no markup.
pub fn copy_text(record: &MessageRecord) -> String {
    let mut text = String::new();
    push_plain(&mut text, &message_spans(record));
    text.trim().to_owned()
}

fn push_plain(text: &mut String, spans: &[Span]) {
    for span in spans {
        match span {
            Span::Text(value) | Span::Code(value) => text.push_str(value),
            Span::CodeBlock { code, .. } => {
                text.push_str(code);
                text.push('\n');
            }
            Span::Bold(children)
            | Span::Italic(children)
            | Span::Strike(children)
            | Span::Underline(children)
            | Span::Colored { children, .. }
            | Span::Quote(children)
            | Span::BlockQuote(children)
            | Span::Link { children, .. } => push_plain(text, children),
            Span::Heading { children, .. } => {
                push_plain(text, children);
                text.push('\n');
            }
            Span::Mention { name } => {
                text.push('@');
                text.push_str(name);
            }
            Span::LineBreak | Span::Rule => text.push('\n'),
            Span::List {
                ordered,
                start,
                items,
            } => {
                for (offset, item) in items.iter().enumerate() {
                    if *ordered {
                        text.push_str(&format!("{}. ", *start as usize + offset));
                    } else {
                        text.push_str("- ");
                    }
                    push_plain(text, item);
                    text.push('\n');
                }
            }
            Span::Table { rows, .. } => {
                for row in rows {
                    for (index, cell) in row.iter().enumerate() {
                        if index > 0 {
                            text.push('\t');
                        }
                        push_plain(text, cell);
                    }
                    text.push('\n');
                }
            }
            Span::Image { .. } => {}
        }
    }
}

/// Own, not deleted. Whether the conversation supports the write is the engine's call.
pub fn can_edit(record: &MessageRecord, my_user_id: &str) -> bool {
    !record.deleted && record.sender_id.as_deref() == Some(my_user_id)
}

pub fn can_delete(record: &MessageRecord, my_user_id: &str) -> bool {
    can_edit(record, my_user_id)
}

fn image_id(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let segments: Vec<&str> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    segments
        .iter()
        .position(|segment| matches!(*segment, "hostedContents" | "objects"))
        .and_then(|index| segments.get(index + 1))
        .or(segments.last())
        .map_or_else(String::new, |segment| (*segment).to_owned())
}

fn dimension(attribute: Option<&str>, style: &str, property: &str) -> Option<u32> {
    let from_style = || {
        style.split(';').find_map(|declaration| {
            let (name, value) = declaration.split_once(':')?;
            (name.trim() == property).then(|| leading_number(value))?
        })
    };
    attribute
        .and_then(leading_number)
        .or_else(from_style)
        .filter(|pixels| *pixels > 0)
}

fn leading_number(text: &str) -> Option<u32> {
    let digits: String = text
        .trim()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// Body spans followed by the flattened text of adaptive-card attachments.
pub fn message_spans(record: &MessageRecord) -> Vec<Span> {
    let mut spans = html_to_spans(&record.body_html);
    for card_text in attachments(record)
        .iter()
        .filter(|attachment| attachment.is_card())
        .filter_map(|attachment| attachment.text.as_deref())
    {
        if !spans.is_empty() {
            spans.push(Span::LineBreak);
        }
        for (index, line) in card_text.lines().enumerate() {
            if index > 0 {
                spans.push(Span::LineBreak);
            }
            spans.push(Span::Text(line.to_owned()));
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    fn record(body_html: &str, attachments_json: &str) -> MessageRecord {
        MessageRecord {
            conversation_id: "c".to_owned(),
            message_id: "m".to_owned(),
            reply_to_id: None,
            sender_id: Some("u".to_owned()),
            sender_name: None,
            created_at: Utc::now(),
            edited_at: None,
            deleted: false,
            body_html: body_html.to_owned(),
            attachments_json: attachments_json.to_owned(),
            reactions_json: "[]".to_owned(),
            mentions_json: "[]".to_owned(),
        }
    }

    #[test]
    fn reaction_without_created_at_still_parses() {
        let mut stored = record("", "[]");
        stored.reactions_json =
            r#"[{"reaction_type":"like","user_id":"u","user_name":null}]"#.to_owned();
        let parsed = reactions(&stored);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].created_at, None);
    }

    #[test]
    fn images_cover_hosted_contents_and_ams_and_skip_emoji() {
        let html = concat!(
            r#"<p>a</p><img src="https://graph.microsoft.com/v1.0/chats/c/messages/1/hostedContents/aWQ9/$value" style="width:300px; height: 120px">"#,
            r#"<img itemtype="http://schema.skype.com/AMSImage" src="https://eu-api.asm.skype.com/v1/objects/0-weu-d1-abc/views/imgo" width="64" height="auto">"#,
            r#"<img itemtype="http://schema.skype.com/Emoji" src="https://x/e.png" alt="x">"#,
            r#"<img src="https://graph.microsoft.com/v1.0/chats/c/messages/1/hostedContents/aWQ9/$value">"#,
        );
        let found = images(&record(html, "[]"));
        assert_eq!(found.len(), 2);
        assert_eq!(
            (found[0].id.as_str(), found[0].width, found[0].height),
            ("aWQ9", Some(300), Some(120))
        );
        assert_eq!(
            (found[1].id.as_str(), found[1].width, found[1].height),
            ("0-weu-d1-abc", Some(64), None)
        );
    }

    #[test]
    fn deleted_messages_have_no_images() {
        let mut deleted = record(r#"<img src="https://x/y">"#, "[]");
        deleted.deleted = true;
        assert!(images(&deleted).is_empty());
    }

    #[test]
    fn edits_keep_images_and_match_files_to_their_attachment_tags() {
        let html = "<p>text</p><p><img src=\"https://graph.microsoft.com/v1.0/chats/c/messages/1/hostedContents/9/$value\"></p><attachment id=\"G1\"></attachment>";
        let attachments = r#"[{"content_type":"reference","name":"a.pdf","url":"https://x/a.pdf","text":null,"id":"G1"}]"#;
        let preserved = edit_preserved(&record(html, attachments));
        assert!(
            preserved
                .images_html
                .starts_with("<p><img src=\"https://graph.microsoft.com/")
        );
        assert!(!preserved.images_html.contains("text"));
        assert_eq!(preserved.files.len(), 1);
        assert_eq!(preserved.files[0].attachment_id, "G1");
        assert_eq!(preserved.files[0].content_url, "https://x/a.pdf");
        assert_eq!(preserved.files[0].name, "a.pdf");
        assert!(preserved.kept.is_empty());
    }

    const QUOTED_HTML: &str =
        "<attachment id=\"Q1\"></attachment><p>answer</p><attachment id=\"F1\"></attachment>";

    #[test]
    fn a_quoted_reply_keeps_its_quote_and_pairs_the_file_by_id() {
        let attachments = r#"[
            {"content_type":"messageReference","name":null,"url":null,"text":null,"id":"Q1","content":"{\"messageId\":\"Q1\",\"messagePreview\":\"hi\"}","quote":{"message_id":"Q1","preview":"hi","sender_name":null}},
            {"content_type":"reference","name":"a.pdf","url":"https://x/a.pdf","text":null,"id":"F1"}
        ]"#;
        let preserved = edit_preserved(&record(QUOTED_HTML, attachments));
        assert_eq!(preserved.files.len(), 1);
        assert_eq!(preserved.files[0].attachment_id, "F1");
        assert_eq!(preserved.kept.len(), 1);
        assert_eq!(preserved.kept[0].id, "Q1");
        assert_eq!(
            preserved.kept[0].content_type.as_deref(),
            Some("messageReference")
        );
        assert!(
            preserved.kept[0]
                .content
                .as_deref()
                .unwrap()
                .contains("messagePreview")
        );
    }

    #[test]
    fn records_cached_before_content_was_stored_leave_the_quote_out() {
        let attachments = r#"[
            {"content_type":"messageReference","name":null,"url":null,"text":null,"quote":{"message_id":"Q1","preview":"hi","sender_name":null}},
            {"content_type":"reference","name":"a.pdf","url":"https://x/a.pdf","text":null}
        ]"#;
        let preserved = edit_preserved(&record(QUOTED_HTML, attachments));
        assert_eq!(preserved.files[0].attachment_id, "F1");
        assert!(preserved.kept.is_empty());
    }

    #[test]
    fn file_kind_follows_the_extension_then_the_content_type() {
        let kind = |name: Option<&str>, content_type: &str| {
            AttachmentInfo {
                content_type: Some(content_type.to_owned()),
                name: name.map(str::to_owned),
                url: None,
                text: None,
                size: None,
                quote: None,
                id: None,
                content: None,
            }
            .file_kind()
        };
        assert_eq!(kind(Some("Plan.DOCX"), "reference"), FileKind::Word);
        assert_eq!(kind(Some("a.xlsx"), "reference"), FileKind::Excel);
        assert_eq!(kind(Some("a.pptx"), "reference"), FileKind::PowerPoint);
        assert_eq!(kind(Some("a.pdf"), "reference"), FileKind::Pdf);
        assert_eq!(kind(Some("a.PNG"), "reference"), FileKind::Image);
        assert_eq!(kind(Some("a.zip"), "reference"), FileKind::Archive);
        assert_eq!(kind(Some("a.bin"), "reference"), FileKind::Other);
        assert_eq!(kind(None, "image/png"), FileKind::Image);
        assert_eq!(kind(Some("noextension"), "reference"), FileKind::Other);
    }

    #[test]
    fn files_expose_name_kind_and_open_url() {
        let json = r#"[{"content_type":"reference","name":"Plan.docx","url":"https://x/Plan.docx","text":null},{"content_type":"messageReference","name":null,"url":null,"text":null}]"#;
        let found = files(&record("", json));
        assert_eq!(found.len(), 1);
        assert_eq!(
            (found[0].name.as_str(), found[0].kind),
            ("Plan.docx", FileKind::Word)
        );
        assert_eq!(found[0].open_url, "https://x/Plan.docx");
        assert_eq!(found[0].size, None);
    }

    #[test]
    fn copy_text_flattens_markup_and_keeps_mentions_and_lines() {
        let html = r#"<p>Hi <at id="0">Ada</at>, <b>look</b></p><p>second <a href="https://x">link</a></p>"#;
        let text = copy_text(&record(html, "[]"));
        assert!(text.starts_with("Hi @Ada, look"), "{text}");
        assert!(text.contains("second link"), "{text}");
        assert!(text.contains('\n'));
    }

    #[test]
    fn old_cached_attachments_still_parse() {
        let parsed = attachments(&record(
            "",
            r#"[{"content_type":"reference","name":"a.pdf","url":"u","text":null}]"#,
        ));
        assert_eq!(parsed.len(), 1);
        assert!(parsed[0].is_file());
        assert_eq!(parsed[0].size, None);
    }

    #[test]
    fn quote_references_are_extracted_from_the_message_reference_attachment() {
        let attachment = graph::Attachment {
            id: Some("1".to_owned()),
            content_type: Some("messageReference".to_owned()),
            content_url: None,
            name: None,
            content: Some(
                r#"{"messageId":"1728","messagePreview":"Hello","messageSender":{"user":{"displayName":"Adele"}}}"#.to_owned(),
            ),
        };
        let info = crate::mapping::attachment_info(&attachment);
        assert_eq!(
            info.quote,
            Some(QuoteInfo {
                message_id: "1728".to_owned(),
                preview: "Hello".to_owned(),
                sender_name: Some("Adele".to_owned())
            })
        );
        let stored = record("", &serde_json::to_string(&[info]).unwrap());
        assert_eq!(quotes(&stored).len(), 1);
    }
}
