use std::sync::Arc;

use chrono::{DateTime, Utc};
use gpui_kit::Image;
use serde::{Deserialize, Serialize};
use teams_core::{
    Draft, FileKind, FileReference, HostedImage, LinkPreview, MentionInput, MessageExtras,
    OBJECT_MARK, UploadedFile,
};

use crate::remote_image::RemoteImage;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineImage {
    pub name: String,
    pub image: Arc<Image>,
    pub dimensions: Option<(u32, u32)>,
}

/// One object in the text: uploaded bytes or a GIF or sticker the message links to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutgoingImage {
    Inline(InlineImage),
    Remote(RemoteImage),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutgoingFile {
    pub name: String,
    pub size: u64,
    pub kind: FileKind,
    pub reference: FileReference,
    pub uploaded: Option<UploadedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplyPreview {
    pub message_id: String,
    pub author: String,
    pub excerpt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditPreview {
    pub message_id: String,
    pub excerpt: String,
    pub scheduled: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    pub draft: Draft,
    pub subject: Option<String>,
    pub mentions: Vec<MentionInput>,
    pub reply: Option<ReplyPreview>,
    pub edit: Option<EditPreview>,
    pub images: Vec<OutgoingImage>,
    pub files: Vec<OutgoingFile>,
    pub link_preview: Option<LinkPreview>,
}

impl Outgoing {
    /// Without list and quote markers, for previews.
    pub fn text(&self) -> String {
        self.draft.plain_text()
    }

    /// The body before mentions become `<at>` tags.
    pub fn html(&self) -> String {
        let mut html = String::new();
        let mut objects = self.images.iter();
        let mut uploaded = 0;
        for character in self.draft.to_html().chars() {
            if character != OBJECT_MARK {
                html.push(character);
                continue;
            }
            match objects.next() {
                Some(OutgoingImage::Inline(_)) => {
                    uploaded += 1;
                    html.push_str(&format!(
                        "<img src=\"../hostedContents/{uploaded}/$value\">"
                    ));
                }
                Some(OutgoingImage::Remote(remote)) => html.push_str(&remote.html()),
                None => {}
            }
        }
        teams_core::emoticon_tags(&html, crate::emoticons::send_tag)
    }

    pub fn has_attachments(&self) -> bool {
        !self.images.is_empty() || !self.files.is_empty()
    }

    pub fn extras(&self) -> MessageExtras {
        MessageExtras {
            kept: Vec::new(),
            images: self
                .images
                .iter()
                .filter_map(|outgoing| match outgoing {
                    OutgoingImage::Inline(inline) => Some(HostedImage {
                        content_type: inline.image.format.mime_type().to_owned(),
                        bytes: Arc::new(inline.image.bytes.clone()),
                    }),
                    OutgoingImage::Remote(_) => None,
                })
                .collect(),
            files: self
                .files
                .iter()
                .map(|file| file.reference.clone())
                .collect(),
        }
    }
}
