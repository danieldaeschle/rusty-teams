use chrono::{FixedOffset, NaiveDate};
use teams_core::ImageRef;

use crate::format;
use crate::rows::{MessageRow, Row};

#[derive(Debug, Clone, PartialEq)]
pub struct GalleryItem {
    pub message_key: String,
    pub image: ImageRef,
    pub sender_id: Option<String>,
    pub author: String,
    pub time: String,
    pub caption: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ViewerRequest {
    pub conversation_id: String,
    pub conversation_title: String,
    pub items: Vec<GalleryItem>,
    pub index: usize,
}

pub fn caption_of(row: &MessageRow) -> String {
    row.blocks
        .iter()
        .filter_map(|block| block.markdown())
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn viewer_time_label(row: &MessageRow, today: NaiveDate, offset: FixedOffset) -> String {
    let label = format::post_time_label(row.created_at, today, offset);
    if row.created_at.with_timezone(&offset).date_naive() == today {
        format!("Today {label}")
    } else {
        label
    }
}

pub fn collect_gallery(rows: &[Row], today: NaiveDate, offset: FixedOffset) -> Vec<GalleryItem> {
    rows.iter()
        .flat_map(Row::messages)
        .filter(|message| !message.deleted)
        .flat_map(|message| {
            let caption = caption_of(message);
            let time = viewer_time_label(message, today, offset);
            message.images.iter().map(move |image| GalleryItem {
                message_key: message.key.clone(),
                image: image.clone(),
                sender_id: message.sender_id.clone(),
                author: message.author.clone(),
                time: time.clone(),
                caption: caption.clone(),
            })
        })
        .collect()
}

pub fn position_of(items: &[GalleryItem], message_key: &str, url: &str) -> Option<usize> {
    items
        .iter()
        .position(|item| item.message_key == message_key && item.image.url == url)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use store::MessageRecord;

    use super::*;
    use crate::rows::{MeetingInfo, RowContext, message_row};
    use crate::translation::TranslationContext;

    fn offset() -> FixedOffset {
        FixedOffset::east_opt(0).unwrap()
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 7).unwrap()
    }

    fn context() -> RowContext {
        RowContext {
            offset: offset(),
            today: today(),
            my_user_id: None,
            names: HashMap::new(),
            pending_reactions: HashMap::new(),
            card_overrides: HashMap::new(),
            translation: TranslationContext::default(),
            skin_tone: 0,
            meeting: MeetingInfo::default(),
        }
    }

    fn record(id: &str, day: u32, body_html: &str) -> MessageRecord {
        MessageRecord {
            conversation_id: "c".into(),
            message_id: id.into(),
            reply_to_id: None,
            sender_id: Some("u".into()),
            sender_name: Some("Mara".into()),
            sender_application_id: None,
            links_json: "[]".to_owned(),
            created_at: Utc.with_ymd_and_hms(2026, 10, day, 13, 40, 0).unwrap(),
            edited_at: None,
            deleted: false,
            body_html: body_html.to_owned(),
            attachments_json: "[]".into(),
            reactions_json: "[]".into(),
            mentions_json: "[]".into(),
            subject: None,
            call_event_json: None,
        }
    }

    fn rows(records: &[MessageRecord]) -> Vec<Row> {
        records
            .iter()
            .map(|record| Row::Message(Box::new(message_row(record, &context()))))
            .collect()
    }

    #[test]
    fn images_come_in_message_order_and_skip_text_only_and_deleted_messages() {
        let mut deleted = record("d", 7, "<img src=\"https://x/deleted\">");
        deleted.deleted = true;
        let records = [
            record(
                "a",
                6,
                "<p>first</p><img src=\"https://x/one\"><img src=\"https://x/two\">",
            ),
            record("b", 6, "<p>no image</p>"),
            deleted,
            record("c", 7, "<img src=\"https://x/three\">"),
        ];
        let items = collect_gallery(&rows(&records), today(), offset());
        let urls: Vec<&str> = items.iter().map(|item| item.image.url.as_str()).collect();
        assert_eq!(urls, ["https://x/one", "https://x/two", "https://x/three"]);
        assert_eq!(items[0].message_key, "a");
        assert_eq!(items[2].message_key, "c");
    }

    #[test]
    fn items_carry_author_caption_and_time() {
        let records = [
            record(
                "a",
                6,
                "<p>Release   notes</p><p>second line</p><img src=\"https://x/one\">",
            ),
            record("b", 7, "<img src=\"https://x/two\">"),
        ];
        let items = collect_gallery(&rows(&records), today(), offset());
        assert_eq!(items[0].author, "Mara");
        assert_eq!(items[0].caption, "Release notes second line");
        assert_eq!(items[0].time, "Yesterday 13:40");
        assert_eq!(items[1].caption, "");
        assert_eq!(items[1].time, "Today 13:40");
    }

    #[test]
    fn position_matches_message_and_url() {
        let records = [
            record("a", 6, "<img src=\"https://x/one\">"),
            record("b", 6, "<img src=\"https://x/one\">"),
        ];
        let items = collect_gallery(&rows(&records), today(), offset());
        assert_eq!(position_of(&items, "b", "https://x/one"), Some(1));
        assert_eq!(position_of(&items, "a", "https://x/one"), Some(0));
        assert_eq!(position_of(&items, "a", "https://x/other"), None);
    }
}
