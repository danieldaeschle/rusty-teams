use std::sync::Arc;

use gpui_kit::{Image, ImageFormat};
use serde::{Deserialize, Serialize};
use store::AttachmentImage;
use teams_core::{Draft, DraftLine, LinkPreview, MentionInput};

use crate::remote_image::{RemoteImage, RemoteKind};
use crate::views::attachment_tray::{InlineImage, OutgoingFile, OutgoingImage};
use crate::views::composer::{Outgoing, ReplyPreview};

const IMAGE_FORMATS: [ImageFormat; 9] = [
    ImageFormat::Png,
    ImageFormat::Jpeg,
    ImageFormat::Webp,
    ImageFormat::Gif,
    ImageFormat::Svg,
    ImageFormat::Bmp,
    ImageFormat::Tiff,
    ImageFormat::Ico,
    ImageFormat::Pnm,
];

#[derive(Serialize, Deserialize)]
enum StoredObject {
    Inline,
    Gif(StoredRemote),
    Sticker(StoredRemote),
}

#[derive(Serialize, Deserialize)]
struct StoredRemote {
    url: String,
    title: String,
    width: u32,
    height: u32,
}

#[derive(Serialize, Deserialize)]
struct StoredPreview {
    url: String,
    title: Option<String>,
    description: Option<String>,
    image_url: Option<String>,
    image_width: Option<u32>,
    image_height: Option<u32>,
}

#[derive(Serialize, Deserialize)]
struct Payload {
    lines: Vec<DraftLine>,
    mentions: Vec<MentionInput>,
    reply: Option<ReplyPreview>,
    files: Vec<OutgoingFile>,
    objects: Vec<StoredObject>,
    preview: Option<StoredPreview>,
}

pub struct StoredOutgoing {
    pub payload: String,
    pub images: Vec<AttachmentImage>,
}

pub fn encode(outgoing: &Outgoing) -> Option<StoredOutgoing> {
    let mut images = Vec::new();
    let mut objects = Vec::new();
    for object in &outgoing.images {
        match object {
            OutgoingImage::Inline(inline) => {
                images.push(AttachmentImage {
                    name: inline.name.clone(),
                    format: inline.image.format.mime_type().to_owned(),
                    bytes: inline.image.bytes.clone(),
                    width: inline.dimensions.map(|(width, _)| width),
                    height: inline.dimensions.map(|(_, height)| height),
                });
                objects.push(StoredObject::Inline);
            }
            OutgoingImage::Remote(remote) => {
                let stored = StoredRemote {
                    url: remote.url.clone(),
                    title: remote.title.clone(),
                    width: remote.width,
                    height: remote.height,
                };
                objects.push(match remote.kind {
                    RemoteKind::Gif => StoredObject::Gif(stored),
                    RemoteKind::Sticker => StoredObject::Sticker(stored),
                });
            }
        }
    }
    let payload = serde_json::to_string(&Payload {
        lines: outgoing.draft.to_lines(),
        mentions: outgoing.mentions.clone(),
        reply: outgoing.reply.clone(),
        files: outgoing.files.clone(),
        objects,
        preview: outgoing.link_preview.as_ref().map(|preview| StoredPreview {
            url: preview.url.clone(),
            title: preview.title.clone(),
            description: preview.description.clone(),
            image_url: preview.image_url.clone(),
            image_width: preview.image_width,
            image_height: preview.image_height,
        }),
    })
    .ok()?;
    Some(StoredOutgoing { payload, images })
}

fn inline_image(stored: &AttachmentImage) -> Option<OutgoingImage> {
    let format = IMAGE_FORMATS
        .into_iter()
        .find(|format| format.mime_type() == stored.format)?;
    Some(OutgoingImage::Inline(InlineImage {
        name: stored.name.clone(),
        image: Arc::new(Image::from_bytes(format, stored.bytes.clone())),
        dimensions: stored.width.zip(stored.height),
    }))
}

fn remote_image(kind: RemoteKind, stored: StoredRemote) -> OutgoingImage {
    OutgoingImage::Remote(RemoteImage {
        kind,
        url: stored.url,
        title: stored.title,
        width: stored.width,
        height: stored.height,
    })
}

pub fn decode(payload: &str, images: &[AttachmentImage]) -> Option<Outgoing> {
    let Payload {
        lines,
        mentions,
        reply,
        files,
        objects,
        preview,
    } = serde_json::from_str(payload).ok()?;
    let mut inline = images.iter();
    let images = objects
        .into_iter()
        .filter_map(|object| match object {
            StoredObject::Inline => inline.next().and_then(inline_image),
            StoredObject::Gif(stored) => Some(remote_image(RemoteKind::Gif, stored)),
            StoredObject::Sticker(stored) => Some(remote_image(RemoteKind::Sticker, stored)),
        })
        .collect();
    Some(Outgoing {
        draft: Draft::from_lines(&lines),
        mentions,
        reply,
        edit: None,
        images,
        files,
        link_preview: preview.map(|preview| LinkPreview {
            url: preview.url,
            title: preview.title,
            description: preview.description,
            image_url: preview.image_url,
            image_width: preview.image_width,
            image_height: preview.image_height,
        }),
    })
}

#[cfg(test)]
mod tests {
    use teams_core::{FileKind, FileReference, MarkKind};

    use super::*;

    fn outgoing() -> Outgoing {
        let mut draft = Draft::plain("hi @Ada Lovelace \u{FFFC}\nsecond");
        draft.toggle(0..2, MarkKind::Bold);
        draft.take_edits();
        Outgoing {
            draft,
            mentions: vec![MentionInput::user("user-1", "Ada Lovelace")],
            reply: Some(ReplyPreview {
                message_id: "message-1".to_owned(),
                author: "Grace".to_owned(),
                excerpt: "earlier".to_owned(),
            }),
            edit: None,
            images: vec![
                OutgoingImage::Inline(InlineImage {
                    name: "shot.png".to_owned(),
                    image: Arc::new(Image::from_bytes(ImageFormat::Png, vec![1, 2, 3])),
                    dimensions: Some((40, 30)),
                }),
                OutgoingImage::Remote(RemoteImage {
                    kind: RemoteKind::Gif,
                    url: "https://example.test/a.gif".to_owned(),
                    title: "wave".to_owned(),
                    width: 100,
                    height: 80,
                }),
            ],
            files: vec![OutgoingFile {
                name: "plan.pdf".to_owned(),
                size: 12,
                kind: FileKind::Pdf,
                reference: FileReference {
                    attachment_id: "attachment-1".to_owned(),
                    content_url: "https://example.test/plan.pdf".to_owned(),
                    name: "plan.pdf".to_owned(),
                },
                uploaded: None,
            }],
            link_preview: Some(LinkPreview {
                url: "https://a.example".to_owned(),
                title: Some("A".to_owned()),
                description: None,
                image_url: None,
                image_width: None,
                image_height: None,
            }),
        }
    }

    #[test]
    fn an_outgoing_message_round_trips_with_formatting_mention_and_image() {
        let original = outgoing();
        let stored = encode(&original).unwrap();
        let restored = decode(&stored.payload, &stored.images).unwrap();
        assert_eq!(restored.draft.text(), original.draft.text());
        assert_eq!(restored.draft.to_html(), original.draft.to_html());
        assert!(restored.draft.to_html().contains("<b>"));
        assert_eq!(restored.mentions, original.mentions);
        assert_eq!(restored.reply, original.reply);
        assert_eq!(restored.images, original.images);
        assert_eq!(restored.files, original.files);
        assert_eq!(restored.link_preview, original.link_preview);
        assert_eq!(restored.edit, None);
    }

    #[test]
    fn an_unreadable_payload_decodes_to_nothing() {
        assert!(decode("not json", &[]).is_none());
    }
}
