use chrono::{DateTime, Local, Utc};
use teams_core::FileCard;

use crate::render::layout_blocks;
use crate::rows::{Delivery, LocalImage, MessageRow, Receipt, Series};
use crate::views::attachment_tray::OutgoingImage;
use crate::views::composer::Outgoing;

pub fn pending_row(
    key: String,
    conversation_id: String,
    my_user_id: Option<String>,
    created_at: DateTime<Utc>,
    outgoing: &Outgoing,
    delivery: Delivery,
) -> MessageRow {
    MessageRow {
        key,
        conversation_id,
        author: "You".to_owned(),
        sender_id: my_user_id,
        application_id: None,
        created_at,
        series: Series::default(),
        time: created_at.with_timezone(&Local).format("%H:%M").to_string(),
        day_header: None,
        blocks: if outgoing.draft.is_blank() && outgoing.images.is_empty() {
            Vec::new()
        } else {
            layout_blocks(&teams_core::html_to_spans(&outgoing.html()))
        },
        edited: false,
        deleted: false,
        reactions: Vec::new(),
        images: outgoing
            .images
            .iter()
            .filter_map(|image| match image {
                OutgoingImage::Remote(remote) => Some(remote.image_ref()),
                OutgoingImage::Inline(_) => None,
            })
            .collect(),
        local_images: outgoing
            .images
            .iter()
            .filter_map(|image| match image {
                OutgoingImage::Inline(inline) => Some(LocalImage {
                    image: inline.image.clone(),
                    size: inline.dimensions,
                }),
                OutgoingImage::Remote(_) => None,
            })
            .collect(),
        adaptive_cards: Vec::new(),
        link_preview: outgoing.link_preview.clone(),
        meeting_link: teams_core::meeting_link_in_html(&outgoing.html()),
        files: outgoing
            .files
            .iter()
            .map(|file| FileCard {
                name: file.name.clone(),
                kind: file.kind,
                content_type: None,
                size: Some(file.size),
                open_url: file.reference.content_url.clone(),
            })
            .collect(),
        subject: outgoing.subject.clone(),
        new_marker: false,
        reply_root: None,
        delivery,
        receipt: Receipt::Hidden,
        own: true,
        forwarded: false,
        translation: None,
    }
}
