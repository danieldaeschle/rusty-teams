use mcp::{
    FileInfo, ForwardedInfo, ImageInfo, ImageKind, LinkInfo, MentionInfo, MessageExtras, QuoteInfo, ReactionInfo,
};
use store::MessageRecord;
use teams_core::MentionTarget;

pub fn message_extras(record: &MessageRecord, my_user_id: Option<&str>) -> MessageExtras {
    MessageExtras {
        text: teams_core::readable_text(record),
        images: teams_core::images(record)
            .into_iter()
            .map(|image| ImageInfo {
                kind: image_kind(image.kind()),
                image_id: image.id,
                alt: image.alt,
            })
            .collect(),
        reactions: teams_core::reaction_summaries(record, my_user_id)
            .into_iter()
            .map(|summary| ReactionInfo {
                emoji: summary.emoji,
                count: summary.count,
                mine: summary.mine,
            })
            .collect(),
        mentions: teams_core::user_mention_inputs(record)
            .into_iter()
            .map(|input| MentionInfo {
                user_id: match input.target {
                    MentionTarget::User { user_id } => Some(user_id),
                    _ => None,
                },
                name: input.text,
            })
            .collect(),
        files: teams_core::files(record)
            .into_iter()
            .map(|file| FileInfo {
                name: file.name,
                size: file.size,
                file_type: Some(file_type(file.kind).to_owned()),
                link: file.open_url,
            })
            .collect(),
        quote: teams_core::message_quote(record).map(|quote| QuoteInfo {
            message_id: quote.message_id,
            sender: quote.sender_name,
            text: quote.preview,
        }),
        forwarded: teams_core::forwards(record)
            .into_iter()
            .map(|forward| ForwardedInfo {
                text: forward.text(),
                message_id: forward.message_id,
                conversation_id: forward.conversation_id,
                sender: forward.sender_name,
                sender_id: forward.sender_id,
                time: forward.sent_at.map(|time| time.to_rfc3339()),
            })
            .collect(),
        cards: teams_core::card_texts(record),
        links: teams_core::link_preview(record)
            .into_iter()
            .map(|preview| LinkInfo {
                url: preview.url,
                title: preview.title,
            })
            .collect(),
    }
}

fn file_type(kind: teams_core::FileKind) -> &'static str {
    match kind {
        teams_core::FileKind::Word => "word",
        teams_core::FileKind::Excel => "excel",
        teams_core::FileKind::PowerPoint => "powerpoint",
        teams_core::FileKind::Pdf => "pdf",
        teams_core::FileKind::Image => "image",
        teams_core::FileKind::Archive => "archive",
        teams_core::FileKind::Other => "other",
    }
}

pub fn image_kind(kind: teams_core::ImageKind) -> ImageKind {
    match kind {
        teams_core::ImageKind::Inline => ImageKind::Inline,
        teams_core::ImageKind::Gif => ImageKind::Gif,
        teams_core::ImageKind::Sticker => ImageKind::Sticker,
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;

    fn record() -> MessageRecord {
        MessageRecord {
            conversation_id: "19:c@thread.v2".to_owned(),
            message_id: "1728".to_owned(),
            sender_id: Some("user-ada".to_owned()),
            sender_name: Some("Ada Example".to_owned()),
            created_at: Utc.with_ymd_and_hms(2026, 10, 6, 9, 0, 0).unwrap(),
            body_html: concat!(
                r#"<attachment id="Q1"></attachment><p>Hi <at id="0">Bob Sample</at>, see this</p>"#,
                r#"<p><img src="https://graph.microsoft.com/v1.0/chats/19:c@thread.v2/messages/1728/hostedContents/aWQ9/$value" alt="a chart" width="300" height="120"></p>"#,
                r#"<p><img src="https://media0.giphy.com/media/abc/giphy.gif"></p>"#,
                r#"<attachment id="F1"></attachment>"#,
            )
            .to_owned(),
            attachments_json: r#"[
                {"content_type":"messageReference","name":null,"url":null,"text":null,"id":"Q1","quote":{"message_id":"1700","preview":"earlier words","sender_name":"Bob Sample"}},
                {"content_type":"reference","name":"Plan.docx","url":"https://example.sharepoint.com/Plan.docx","text":null,"id":"F1"},
                {"content_type":"application/vnd.microsoft.card.adaptive","name":null,"url":null,"text":"fallback","content":"{\"type\":\"AdaptiveCard\",\"version\":\"1.4\",\"body\":[{\"type\":\"TextBlock\",\"text\":\"Build passed\"}]}"}
            ]"#
            .to_owned(),
            reactions_json: r#"[
                {"reaction_type":"like","user_id":"user-me","user_name":"Me"},
                {"reaction_type":"like","user_id":"user-bob","user_name":"Bob Sample"},
                {"reaction_type":"heart","user_id":"user-bob","user_name":"Bob Sample"}
            ]"#
            .to_owned(),
            mentions_json: r#"[{"user_id":"user-bob","name":"Bob","id":0,"target_id":"user-bob"},{"user_id":"user-bob","name":"Sample","id":1,"target_id":"user-bob"}]"#.to_owned(),
            links_json: r#"[{"@type":"http://schema.skype.com/HyperLink","itemid":"0","url":"https://news.example.com/story","preview":{"title":"A story","description":"Words"},"previewenabled":true}]"#.to_owned(),
            ..MessageRecord::default()
        }
    }

    #[test]
    fn extras_cover_every_part_of_a_real_shaped_message() {
        let extras = message_extras(&record(), Some("user-me"));
        assert!(
            extras.text.starts_with("Hi @Bob Sample, see this"),
            "{}",
            extras.text
        );
        assert!(!extras.text.contains("earlier words"), "{}", extras.text);
        let images: Vec<_> = extras
            .images
            .iter()
            .map(|image| (image.kind, image.alt.as_deref()))
            .collect();
        assert_eq!(
            images,
            [(ImageKind::Inline, Some("a chart")), (ImageKind::Gif, None)]
        );
        assert_ne!(extras.images[0].image_id, extras.images[1].image_id);
        assert_eq!(
            extras.reactions,
            [
                ReactionInfo {
                    emoji: "\u{1F44D}".to_owned(),
                    count: 2,
                    mine: true
                },
                ReactionInfo {
                    emoji: "\u{2764}".to_owned(),
                    count: 1,
                    mine: false
                },
            ]
        );
        assert_eq!(
            extras.mentions,
            [MentionInfo {
                name: "Bob Sample".to_owned(),
                user_id: Some("user-bob".to_owned())
            }]
        );
        assert_eq!(extras.files.len(), 1);
        assert_eq!(extras.files[0].name, "Plan.docx");
        assert_eq!(extras.files[0].file_type.as_deref(), Some("word"));
        assert_eq!(
            extras.quote,
            Some(QuoteInfo {
                message_id: "1700".to_owned(),
                sender: Some("Bob Sample".to_owned()),
                text: "earlier words".to_owned(),
            })
        );
        assert_eq!(extras.cards, ["Build passed"]);
        assert_eq!(
            extras.links,
            [LinkInfo {
                url: "https://news.example.com/story".to_owned(),
                title: Some("A story".to_owned())
            }]
        );
    }

    #[test]
    fn an_inline_reply_blockquote_becomes_the_quote() {
        let stored = MessageRecord {
            body_html: r#"<blockquote itemscope="" itemtype="http://schema.skype.com/Reply" itemid="1700"><strong itemprop="mri">Bob Sample</strong><p itemprop="preview">earlier words</p></blockquote><p>answer</p>"#.to_owned(),
            ..MessageRecord::default()
        };
        let extras = message_extras(&stored, None);
        assert_eq!(extras.text, "answer");
        assert_eq!(
            extras.quote,
            Some(QuoteInfo {
                message_id: "1700".to_owned(),
                sender: Some("Bob Sample".to_owned()),
                text: "earlier words".to_owned(),
            })
        );
    }

    #[test]
    fn a_plain_message_has_no_extras() {
        let plain = MessageRecord {
            body_html: "<p>hello</p>".to_owned(),
            attachments_json: "[]".to_owned(),
            reactions_json: "[]".to_owned(),
            mentions_json: "[]".to_owned(),
            links_json: "[]".to_owned(),
            ..MessageRecord::default()
        };
        let extras = message_extras(&plain, None);
        assert_eq!(extras.text, "hello");
        assert_eq!(
            MessageExtras {
                text: String::new(),
                ..extras
            },
            MessageExtras::default()
        );
    }

    #[test]
    fn a_forward_lists_the_original_message_apart_from_the_text() {
        let forwarded = MessageRecord {
            body_html: r#"<p>fyi</p><attachment id="9"></attachment>"#.to_owned(),
            attachments_json: serde_json::json!([{
                "content_type": "forwardedMessageReference",
                "name": null, "url": null, "text": null, "id": "9",
                "content": serde_json::json!({
                    "originalMessageId": "9",
                    "originalMessageContent": "<p>original <b>words</b></p>",
                    "originalConversationId": "19:origin@thread.v2",
                    "originalSentDateTime": "2026-10-06T08:00:00.000+00:00",
                    "originalMessageSender": {"user": {"id": "user-cleo", "displayName": null}}
                }).to_string()
            }])
            .to_string(),
            ..record()
        };
        let extras = message_extras(&forwarded, None);
        assert_eq!(extras.text, "fyi");
        assert_eq!(
            extras.forwarded,
            [ForwardedInfo {
                message_id: "9".to_owned(),
                conversation_id: Some("19:origin@thread.v2".to_owned()),
                sender: None,
                sender_id: Some("user-cleo".to_owned()),
                time: Some("2026-10-06T08:00:00+00:00".to_owned()),
                text: "original words".to_owned(),
            }]
        );
    }
}
