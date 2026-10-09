use chrono::{DateTime, Utc};
use scraper::{ElementRef, Html};
use serde::{Deserialize, Serialize};
use store::MessageRecord;

use crate::adaptive_card::{AdaptiveCard, card_content_text};
use crate::markdown::escape_html;
use crate::mentions::MentionInput;
use crate::spans::{Span, html_to_spans};

const EMOJI_ITEMTYPE: &str = "schema.skype.com/Emoji";
const REPLY_ITEMTYPE: &str = "schema.skype.com/Reply";

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

    pub fn adaptive_card(&self) -> Option<AdaptiveCard> {
        self.content
            .as_deref()
            .filter(|_| self.is_card())
            .and_then(AdaptiveCard::parse)
    }

    pub fn card_text(&self) -> Option<String> {
        let content = self.content.as_deref().filter(|_| self.is_card())?;
        card_content_text(content).or_else(|| self.text.clone())
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
    #[serde(default)]
    pub id: Option<i64>,
    #[serde(default)]
    pub target_id: Option<String>,
    #[serde(default)]
    pub group: bool,
}

pub fn attachments(record: &MessageRecord) -> Vec<AttachmentInfo> {
    serde_json::from_str(&record.attachments_json).unwrap_or_default()
}

pub fn adaptive_cards(record: &MessageRecord) -> Vec<AdaptiveCard> {
    attachments(record)
        .iter()
        .filter_map(AttachmentInfo::adaptive_card)
        .collect()
}

pub fn card_texts(record: &MessageRecord) -> Vec<String> {
    attachments(record)
        .iter()
        .filter_map(AttachmentInfo::card_text)
        .collect()
}

pub fn reactions(record: &MessageRecord) -> Vec<ReactionInfo> {
    serde_json::from_str(&record.reactions_json).unwrap_or_default()
}

pub fn mentions(record: &MessageRecord) -> Vec<MentionInfo> {
    let mut infos: Vec<MentionInfo> =
        serde_json::from_str(&record.mentions_json).unwrap_or_default();
    for (index, info) in infos.iter_mut().enumerate() {
        info.id = info.id.or(i64::try_from(index).ok());
        info.target_id = info.target_id.take().or_else(|| info.user_id.clone());
    }
    infos
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
    for card_text in card_texts(record) {
        text.push('\n');
        text.push_str(&card_text);
    }
    text.trim().to_owned()
}

pub(crate) fn push_plain(text: &mut String, spans: &[Span]) {
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
            | Span::Superscript(children)
            | Span::Subscript(children)
            | Span::Sized(_, children)
            | Span::Underline(children)
            | Span::Colored { children, .. }
            | Span::Quote(children)
            | Span::BlockQuote(children)
            | Span::Link { children, .. } => push_plain(text, children),
            Span::Heading { children, .. } => {
                push_plain(text, children);
                text.push('\n');
            }
            Span::Mention { name, .. } => {
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

pub fn message_spans(record: &MessageRecord) -> Vec<Span> {
    spans_with_mentions(record, &mentions(record))
}

fn spans_with_mentions(record: &MessageRecord, infos: &[MentionInfo]) -> Vec<Span> {
    let mut spans = html_to_spans(&with_inline_quotes(record));
    merge_split_mentions(&mut spans, infos);
    spans
}

fn with_inline_quotes(record: &MessageRecord) -> String {
    let mut html = record.body_html.clone();
    for attachment in attachments(record) {
        let Some(quote) = attachment.quote else {
            continue;
        };
        let id = attachment.id.unwrap_or_else(|| quote.message_id.clone());
        let tag = format!("<attachment id=\"{id}\"></attachment>");
        let block = quote_html(&quote);
        if html.contains(&tag) {
            html = html.replacen(&tag, &block, 1);
        } else {
            html.insert_str(0, &block);
        }
    }
    html
}

fn quote_html(quote: &QuoteInfo) -> String {
    let sender = quote
        .sender_name
        .as_deref()
        .map(|name| format!("<strong itemprop=\"mri\">{}</strong>", escape_html(name)))
        .unwrap_or_default();
    format!(
        "<blockquote itemscope=\"\" itemtype=\"http://{REPLY_ITEMTYPE}\" itemid=\"{}\">{sender}<p itemprop=\"preview\">{}</p></blockquote>",
        escape_html(&quote.message_id),
        escape_html(&quote.preview),
    )
}

pub fn user_mention_inputs(record: &MessageRecord) -> Vec<MentionInput> {
    let infos = mentions(record);
    let mut inputs = Vec::new();
    collect_user_mentions(&spans_with_mentions(record, &infos), &infos, &mut inputs);
    inputs
}

fn collect_user_mentions(spans: &[Span], infos: &[MentionInfo], inputs: &mut Vec<MentionInput>) {
    for span in spans {
        match span {
            Span::Bold(children)
            | Span::Italic(children)
            | Span::Strike(children)
            | Span::Superscript(children)
            | Span::Subscript(children)
            | Span::Sized(_, children)
            | Span::Underline(children)
            | Span::Colored { children, .. }
            | Span::Heading { children, .. }
            | Span::Quote(children)
            | Span::BlockQuote(children)
            | Span::Link { children, .. } => collect_user_mentions(children, infos, inputs),
            Span::List { items, .. } => items
                .iter()
                .for_each(|item| collect_user_mentions(item, infos, inputs)),
            Span::Table { rows, .. } => rows
                .iter()
                .flatten()
                .for_each(|cell| collect_user_mentions(cell, infos, inputs)),
            Span::Mention { name, id } => {
                let parsed: Option<i64> = id.as_deref().and_then(|id| id.parse().ok());
                let user_id = infos
                    .iter()
                    .find(|info| parsed.is_some() && info.id == parsed)
                    .and_then(|info| info.user_id.as_deref());
                if let Some(user_id) = user_id {
                    inputs.push(MentionInput::user(user_id, name));
                }
            }
            _ => {}
        }
    }
}

fn merge_split_mentions(spans: &mut Vec<Span>, mentions: &[MentionInfo]) {
    let target_of = |id: &Option<String>| -> Option<String> {
        let parsed: i64 = id.as_deref()?.parse().ok()?;
        mentions
            .iter()
            .find(|info| info.id == Some(parsed))?
            .target_id
            .clone()
    };
    let mut merged: Vec<Span> = Vec::with_capacity(spans.len());
    for mut span in std::mem::take(spans) {
        match &mut span {
            Span::Bold(children)
            | Span::Italic(children)
            | Span::Strike(children)
            | Span::Superscript(children)
            | Span::Subscript(children)
            | Span::Sized(_, children)
            | Span::Underline(children)
            | Span::Colored { children, .. }
            | Span::Heading { children, .. }
            | Span::Quote(children)
            | Span::BlockQuote(children)
            | Span::Link { children, .. } => merge_split_mentions(children, mentions),
            Span::List { items, .. } => items
                .iter_mut()
                .for_each(|item| merge_split_mentions(item, mentions)),
            Span::Table { rows, .. } => rows
                .iter_mut()
                .flatten()
                .for_each(|cell| merge_split_mentions(cell, mentions)),
            Span::Mention { name, id } => {
                let continues = match merged.as_slice() {
                    [
                        ..,
                        Span::Mention {
                            id: previous_id, ..
                        },
                        Span::Text(gap),
                    ] => {
                        gap.trim().is_empty()
                            && target_of(id).is_some()
                            && target_of(previous_id) == target_of(id)
                    }
                    _ => false,
                };
                if continues {
                    merged.pop();
                    if let Some(Span::Mention {
                        name: previous_name,
                        ..
                    }) = merged.last_mut()
                    {
                        previous_name.push(' ');
                        previous_name.push_str(name);
                    }
                    continue;
                }
            }
            _ => {}
        }
        merged.push(span);
    }
    *spans = merged;
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
            sender_application_id: None,
            links_json: "[]".to_owned(),
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

    #[test]
    fn a_message_reference_renders_as_a_reply_quote_where_its_tag_sits() {
        let json = r#"[{"content_type":"messageReference","name":null,"url":null,"text":null,"id":"Q1","quote":{"message_id":"Q1","preview":"a < b","sender_name":"Ada"}}]"#;
        let stored = record("<p></p><attachment id=\"Q1\"></attachment>\ntest<p></p>", json);
        let spans = message_spans(&stored);
        let Some(Span::Quote(children)) = spans.first() else {
            panic!("expected a reply quote first, got {spans:?}");
        };
        let mut quoted = String::new();
        push_plain(&mut quoted, children);
        assert!(quoted.contains("Ada"));
        assert!(quoted.contains("a < b"));
        let mut own = String::new();
        push_plain(&mut own, &spans[1..]);
        assert_eq!(own.trim(), "test");
    }

    #[test]
    fn a_message_reference_without_its_tag_still_shows_the_quote() {
        let json = r#"[{"content_type":"messageReference","name":null,"url":null,"text":null,"quote":{"message_id":"Q1","preview":"hi","sender_name":null}}]"#;
        let spans = message_spans(&record("<p>answer</p>", json));
        assert!(matches!(spans.first(), Some(Span::Quote(_))));
    }

    fn with_mentions(body_html: &str, mentions_json: &str) -> MessageRecord {
        let mut stored = record(body_html, "[]");
        stored.mentions_json = mentions_json.to_owned();
        stored
    }

    fn mention_names(spans: &[Span]) -> Vec<&str> {
        spans
            .iter()
            .filter_map(|span| match span {
                Span::Mention { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_tag_mention_split_per_word_merges_into_one() {
        let json = r#"[
            {"user_id":null,"name":"Tip","id":0,"target_id":"T1"},
            {"user_id":null,"name":"of","id":1,"target_id":"T1"},
            {"user_id":null,"name":"the","id":2,"target_id":"T1"},
            {"user_id":null,"name":"day","id":3,"target_id":"T1"}
        ]"#;
        let html = r#"<p>Hey <at id="0">Tip</at> <at id="1">of</at> <at id="2">the</at> <at id="3">day</at>!</p>"#;
        let spans = message_spans(&with_mentions(html, json));
        assert_eq!(mention_names(&spans), vec!["Tip of the day"]);
        assert_eq!(
            copy_text(&with_mentions(html, json)),
            "Hey @Tip of the day!"
        );
    }

    #[test]
    fn a_two_word_user_name_merges_but_different_users_stay_apart() {
        let json = r#"[
            {"user_id":"U1","name":"Ada","id":0,"target_id":"U1"},
            {"user_id":"U1","name":"Lovelace","id":1,"target_id":"U1"},
            {"user_id":"U2","name":"Bob","id":2,"target_id":"U2"}
        ]"#;
        let html = r#"<at id="0">Ada</at> <at id="1">Lovelace</at> <at id="2">Bob</at>"#;
        let spans = message_spans(&with_mentions(html, json));
        assert_eq!(mention_names(&spans), vec!["Ada Lovelace", "Bob"]);
    }

    #[test]
    fn different_users_next_to_each_other_stay_two_mentions() {
        let json = r#"[
            {"user_id":"U1","name":"Ada","id":0,"target_id":"U1"},
            {"user_id":"U2","name":"Bob","id":1,"target_id":"U2"}
        ]"#;
        let html = r#"<at id="0">Ada</at> <at id="1">Bob</at>"#;
        let spans = message_spans(&with_mentions(html, json));
        assert_eq!(mention_names(&spans), vec!["Ada", "Bob"]);
    }

    #[test]
    fn nested_split_mentions_merge() {
        let json = r#"[
            {"user_id":"U1","name":"Ada","id":0,"target_id":"U1"},
            {"user_id":"U1","name":"Lovelace","id":1,"target_id":"U1"}
        ]"#;
        let html = r#"<b><at id="0">Ada</at> <at id="1">Lovelace</at></b>"#;
        let spans = message_spans(&with_mentions(html, json));
        let [Span::Bold(children)] = spans.as_slice() else {
            panic!("bold expected: {spans:?}")
        };
        assert_eq!(mention_names(children), vec!["Ada Lovelace"]);
    }

    #[test]
    fn old_mention_json_without_ids_merges_users_by_position() {
        let json = r#"[{"user_id":"U1","name":"Ada"},{"user_id":"U1","name":"Lovelace"},{"user_id":null,"name":"Tip"},{"user_id":null,"name":"day"}]"#;
        let html = r#"<at id="0">Ada</at> <at id="1">Lovelace</at> <at id="2">Tip</at> <at id="3">day</at>"#;
        let stored = with_mentions(html, json);
        let parsed = mentions(&stored);
        assert_eq!(
            (parsed[1].id, parsed[1].target_id.as_deref()),
            (Some(1), Some("U1"))
        );
        assert_eq!(
            mention_names(&message_spans(&stored)),
            vec!["Ada Lovelace", "Tip", "day"]
        );
    }

    #[test]
    fn edit_mentions_follow_the_merged_names() {
        let json = r#"[
            {"user_id":"U1","name":"Ada","id":0,"target_id":"U1"},
            {"user_id":"U1","name":"Lovelace","id":1,"target_id":"U1"},
            {"user_id":"U2","name":"Bob","id":2,"target_id":"U2"}
        ]"#;
        let html = r#"<at id="0">Ada</at> <at id="1">Lovelace</at> and <at id="2">Bob</at>"#;
        assert_eq!(
            user_mention_inputs(&with_mentions(html, json)),
            vec![
                MentionInput::user("U1", "Ada Lovelace"),
                MentionInput::user("U2", "Bob"),
            ]
        );
    }
}
