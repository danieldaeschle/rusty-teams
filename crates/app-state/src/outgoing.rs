use std::sync::Arc;

use chrono::{DateTime, Utc};
use gpui_kit::Image;
use serde::{Deserialize, Serialize};
use teams_core::{
    AnnouncementBanner, Draft, FileKind, FileReference, HostedImage, LinkPreview, MentionInput,
    MessageExtras, OBJECT_MARK, UploadedFile,
};

use store::MessageImportance;

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
    pub importance: MessageImportance,
    pub banner: Option<AnnouncementBanner>,
}

/// Teams shows a code block's language and Copy button only when a marker paragraph links to it.
/// The server adds the `x_` that Teams' own messages show on the marker's id.
fn link_code_blocks(html: &str) -> String {
    let mut linked = String::with_capacity(html.len());
    for (index, part) in html.split("<pre ").enumerate() {
        if index > 0 {
            // Derived from the body, so every html() of one message gives the same text.
            let id = code_block_id(html, index);
            linked.push_str(&format!(
                "<p itemtype=\"{}\" id=\"codeBlockEditor-{id}\">&nbsp;</p><pre itemid=\"codeBlockEditor-{id}\" ",
                teams_core::CODE_BLOCK_ITEMTYPE
            ));
        }
        linked.push_str(part);
    }
    linked
}

fn code_block_id(html: &str, index: usize) -> uuid::Uuid {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::hash::DefaultHasher::new();
    (html, index).hash(&mut hasher);
    let high = hasher.finish();
    high.hash(&mut hasher);
    uuid::Uuid::from_u64_pair(high, hasher.finish())
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
        link_code_blocks(&teams_core::emoticon_tags(
            &html,
            crate::emoticons::send_tag,
        ))
    }

    pub fn has_attachments(&self) -> bool {
        !self.images.is_empty() || !self.files.is_empty()
    }

    pub fn extras(&self) -> MessageExtras {
        MessageExtras {
            importance: self.importance.stored(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_code_block_gets_its_own_teams_marker() {
        let html = "<p>a</p><pre class=\"language-rs\"><code>x</code></pre><pre class=\"language-py\"><code>&lt;pre y</code></pre>";
        let linked = link_code_blocks(html);
        let ids: Vec<&str> = linked
            .split("itemid=\"codeBlockEditor-")
            .skip(1)
            .map(|rest| &rest[..36])
            .collect();
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1]);
        for id in &ids {
            assert!(linked.contains(&format!(
                "<p itemtype=\"{}\" id=\"codeBlockEditor-{id}\">&nbsp;</p><pre itemid=\"codeBlockEditor-{id}\" class=",
                teams_core::CODE_BLOCK_ITEMTYPE
            )));
        }
        assert!(linked.contains("&lt;pre y"));
        assert_eq!(link_code_blocks(html), linked);
        assert_eq!(link_code_blocks("<p>no code</p>"), "<p>no code</p>");
    }
}
