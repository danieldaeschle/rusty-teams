use super::rules::{ChatKind, Preview};
use super::stack::ToastModel;
use crate::format;

const PREVIEW_OFF: &str = "New message";
const IMAGE_LABEL: &str = "Image";
const UNKNOWN_SENDER: &str = "Unknown";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToastText {
    pub title: String,
    pub subtitle: String,
    pub preview: String,
    pub image_only: bool,
}

impl ToastText {
    pub fn narration(&self) -> String {
        format!("{}, {}, {}", self.title, self.subtitle, self.preview)
    }
}

pub fn describe(model: &ToastModel, preview_on: bool) -> ToastText {
    let sender = if model.sender_name.trim().is_empty() {
        UNKNOWN_SENDER
    } else {
        model.sender_name.as_str()
    };
    let (title, subtitle) = match &model.kind {
        ChatKind::Direct => (sender.to_owned(), "Direct".to_owned()),
        ChatKind::Group { member_count } => (
            model.chat_title.clone(),
            format!("Group, {member_count} people"),
        ),
        ChatKind::Channel { team, channel } => (sender.to_owned(), format!("{team} > {channel}")),
    };
    let body = match (&model.preview, preview_on) {
        (_, false) => PREVIEW_OFF.to_owned(),
        (Preview::Text(text), true) => text.clone(),
        (Preview::Image, true) => IMAGE_LABEL.to_owned(),
    };
    let prefixed = preview_on && model.kind != ChatKind::Direct;
    let preview = if prefixed {
        format!("{}: {body}", format::first_name(sender))
    } else {
        body
    };
    ToastText {
        title,
        subtitle,
        preview,
        image_only: preview_on && model.preview == Preview::Image,
    }
}

/// Byte range of the first word after each `@`, for highlighting mentions.
pub fn mention_ranges(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    let mut search_from = 0;
    while let Some(offset) = text[search_from..].find('@') {
        let start = search_from + offset;
        let end = text[start + 1..]
            .char_indices()
            .find(|(_, character)| !character.is_alphanumeric())
            .map_or(text.len(), |(index, _)| start + 1 + index);
        if end > start + 1 {
            ranges.push(start..end);
        }
        search_from = end.max(start + 1);
    }
    ranges
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::notify::stack::{ReplyState, ToastTimer};

    fn model(kind: ChatKind, preview: Preview) -> ToastModel {
        ToastModel {
            id: 1,
            conversation_id: "c".into(),
            message_id: "m".into(),
            kind,
            chat_title: "Retro-Team".into(),
            sender_id: None,
            sender_name: "Priya Nair".into(),
            preview,
            mentions_me: false,
            count: 1,
            timer: ToastTimer::new(Instant::now(), false),
            reply: ReplyState::Closed,
            reply_text: String::new(),
            hovered: false,
            time: "14:30".into(),
        }
    }

    #[test]
    fn direct_shows_sender_and_plain_preview() {
        let text = describe(&model(ChatKind::Direct, Preview::Text("Hello".into())), true);
        assert_eq!(text.title, "Priya Nair");
        assert_eq!(text.subtitle, "Direct");
        assert_eq!(text.preview, "Hello");
    }

    #[test]
    fn group_shows_chat_title_and_prefixes_sender() {
        let text = describe(
            &model(ChatKind::Group { member_count: 5 }, Preview::Text("Hi".into())),
            true,
        );
        assert_eq!(text.title, "Retro-Team");
        assert_eq!(text.subtitle, "Group, 5 people");
        assert_eq!(text.preview, "Priya: Hi");
    }

    #[test]
    fn channel_shows_path_and_prefixes_sender() {
        let kind = ChatKind::Channel {
            team: "Platform".into(),
            channel: "Releases".into(),
        };
        let text = describe(&model(kind, Preview::Text("Go".into())), true);
        assert_eq!(text.title, "Priya Nair");
        assert_eq!(text.subtitle, "Platform > Releases");
        assert_eq!(text.preview, "Priya: Go");
    }

    #[test]
    fn preview_off_hides_content_but_keeps_name() {
        let text = describe(&model(ChatKind::Direct, Preview::Text("geheim".into())), false);
        assert_eq!(text.preview, "New message");
        assert!(!text.narration().contains("geheim"));
    }

    #[test]
    fn image_only_is_flagged() {
        let text = describe(&model(ChatKind::Direct, Preview::Image), true);
        assert_eq!(text.preview, "Image");
        assert!(text.image_only);
    }

    #[test]
    fn mention_ranges_cover_first_word_after_at() {
        let text = "Ping @Jonas can you ask @Mara @";
        let found: Vec<&str> = mention_ranges(text).into_iter().map(|range| &text[range]).collect();
        assert_eq!(found, ["@Jonas", "@Mara"]);
    }
}
