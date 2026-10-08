use graph::Message;
use scraper::Html;
use store::{ChatPreview, MessageRecord};

const MAX_PREVIEW_CHARS: usize = 200;

pub fn preview_text(html: &str) -> Option<String> {
    let fragment = Html::parse_fragment(html);
    let collapsed = fragment
        .root_element()
        .text()
        .flat_map(str::split_whitespace)
        .collect::<Vec<_>>()
        .join(" ");
    if collapsed.is_empty() {
        return None;
    }
    Some(collapsed.chars().take(MAX_PREVIEW_CHARS).collect())
}

pub(crate) struct OwnedPreview {
    pub text: Option<String>,
    pub sender_id: Option<String>,
    pub sender_name: Option<String>,
    pub deleted: bool,
    pub system: bool,
}

impl OwnedPreview {
    pub(crate) fn from_graph(message: &Message) -> Self {
        let regular = message
            .message_type
            .as_deref()
            .is_none_or(|kind| kind == "message");
        let deleted = message.is_deleted();
        let text = message
            .body
            .as_ref()
            .and_then(|body| body.content.as_deref())
            .filter(|_| regular && !deleted)
            .and_then(preview_text);
        let sender = message.from.as_ref();
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
        }
    }

    pub(crate) fn from_record(record: &MessageRecord) -> Self {
        OwnedPreview {
            text: if record.deleted {
                None
            } else {
                preview_text(&record.body_html)
            },
            sender_id: record.sender_id.clone(),
            sender_name: record.sender_name.clone(),
            deleted: record.deleted,
            system: false,
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
}
