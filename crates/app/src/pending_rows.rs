use chrono::{DateTime, Local, Utc};
use teams_core::{FileCard, QuoteInfo};

use crate::render::{Block, layout_blocks};
use crate::rows::{Delivery, LocalImage, MessageRow, Receipt, Series};
use crate::views::attachment_tray::OutgoingImage;
use crate::views::composer::Outgoing;

fn pending_blocks(outgoing: &Outgoing) -> Vec<Block> {
    let quote = outgoing.reply.as_ref().map(|reply| {
        teams_core::quote_html(&QuoteInfo {
            message_id: reply.message_id.clone(),
            preview: reply.excerpt.clone(),
            sender_name: Some(reply.author.clone()),
        })
    });
    if quote.is_none() && outgoing.draft.is_blank() && outgoing.images.is_empty() {
        return Vec::new();
    }
    let html = quote.unwrap_or_default() + &outgoing.html();
    layout_blocks(&teams_core::html_to_spans(&html))
}

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
        blocks: pending_blocks(outgoing),
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
        call: None,
    }
}

#[cfg(test)]
mod tests {
    use teams_core::Draft;

    use super::*;
    use crate::views::composer::ReplyPreview;

    #[test]
    fn a_pending_reply_shows_its_quote_before_the_server_echo() {
        let outgoing = Outgoing {
            draft: Draft::plain("agreed"),
            subject: None,
            mentions: Vec::new(),
            reply: Some(ReplyPreview {
                message_id: "message-1".to_owned(),
                author: "Grace".to_owned(),
                excerpt: "earlier".to_owned(),
            }),
            edit: None,
            images: Vec::new(),
            files: Vec::new(),
            link_preview: None,
        };
        let row = pending_row(
            "key".to_owned(),
            "chat".to_owned(),
            None,
            Utc::now(),
            &outgoing,
            Delivery::Sending,
        );
        assert!(matches!(row.blocks.first(), Some(Block::Reply { .. })));
        assert_eq!(row.blocks.len(), 2);
    }
}
