use chatsvc::ConversationRef;
use chatsvc::ams::AmsImage;
use chrono::{DateTime, Utc};
use graph::{FileReference, HostedImage, MessageExtras, OutgoingMention};
use serde_json::{Value, json};

use crate::engine::SyncEngine;
use crate::error::{Error, Result};
use crate::remote::Remote;
use crate::teams_quote::{TeamsQuote, chatsvc_mention_html, message_properties};

const AMS_IMAGE_ITEMTYPE: &str = "http://schema.skype.com/AMSImage";
const FILE_ITEMTYPE: &str = "http://schema.skype.com/File";
const HOSTED_PREFIX: &str = "../hostedContents/";
const HOSTED_SUFFIX: &str = "/$value";
const SITE_SEGMENTS: [&str; 2] = ["/personal/", "/sites/"];

/// What was posted, in Graph form for the cache, and the html the cache keeps.
pub(crate) struct ChatsvcSent {
    pub message: graph::Message,
    pub html: String,
}

impl<R: Remote> SyncEngine<R> {
    /// Posts like Teams web: mention spans, AMS images and the `files` property; `quote` puts a reply quote first.
    pub(crate) async fn send_via_chatsvc(
        &self,
        conversation: &ConversationRef,
        root_id: Option<&str>,
        html: &str,
        mentions: &[OutgoingMention],
        extras: &MessageExtras,
        quote: Option<&TeamsQuote>,
    ) -> Result<ChatsvcSent> {
        let properties = message_properties(
            quote,
            mentions,
            extras.importance,
            &files_property(&extras.files),
        )
        .ok_or(Error::Unsupported(
            "a team, channel or tag mention in a chat",
        ))?;
        self.ensure_display_name().await?;
        let me = self
            .me()
            .ok_or(Error::Unsupported("sending before sign-in"))?;
        let conversation_id = conversation.conversation_id();
        let mut uploaded = Vec::new();
        for image in &extras.images {
            uploaded.push(
                self.remote
                    .upload_ams_image(&conversation_id, &image.bytes, &image_file_name(image))
                    .await?,
            );
        }
        let body = with_ams_images(html, &uploaded);
        let quote_html = quote.map(|quote| quote.html.as_str()).unwrap_or_default();
        let content = format!("{quote_html}{}", chatsvc_mention_html(&body, mentions));
        let message_id = self
            .remote
            .post_chatsvc_message(conversation, &content, &me.display_name, &properties)
            .await?;
        let created_at = message_id
            .parse::<i64>()
            .ok()
            .and_then(DateTime::from_timestamp_millis)
            .unwrap_or_else(Utc::now);
        let cached_html = format!("{quote_html}{body}");
        let attachments: Vec<Value> = extras
            .files
            .iter()
            .map(|file| {
                json!({
                    "id": file.attachment_id,
                    "contentType": "reference",
                    "contentUrl": file.content_url,
                    "name": file.name,
                })
            })
            .collect();
        let message = serde_json::from_value(json!({
            "id": message_id,
            "messageType": "message",
            "createdDateTime": created_at.to_rfc3339(),
            "replyToId": root_id,
            "importance": extras.importance,
            "from": {"user": {"id": me.user_id, "displayName": me.display_name}},
            "body": {"contentType": "html", "content": cached_html},
            "mentions": mentions.iter().map(OutgoingMention::to_json).collect::<Vec<_>>(),
            "attachments": attachments,
        }))
        .map_err(|_| Error::Unsupported("a chatsvc send without a message shape"))?;
        Ok(ChatsvcSent {
            message,
            html: cached_html,
        })
    }
}

fn image_file_name(image: &HostedImage) -> String {
    let extension = image
        .content_type
        .rsplit('/')
        .next()
        .filter(|extension| !extension.is_empty())
        .unwrap_or("png");
    format!("image.{extension}")
}

/// Swaps each Graph `../hostedContents/{n}/$value` image for its AMS upload; images not placed in the text go last.
fn with_ams_images(html: &str, uploaded: &[AmsImage]) -> String {
    let mut body = html.to_owned();
    for (index, image) in uploaded.iter().enumerate() {
        let tag = format!(
            "<img itemtype=\"{AMS_IMAGE_ITEMTYPE}\" itemscope=\"\" src=\"{}\" itemid=\"{}\" alt=\"image\">",
            image.url, image.object_id
        );
        let placeholder = format!("{HOSTED_PREFIX}{}{HOSTED_SUFFIX}", index + 1);
        match image_tag_around(&body, &placeholder) {
            Some((start, end)) => body.replace_range(start..end, &tag),
            None => body.push_str(&format!("<p>{tag}</p>")),
        }
    }
    body
}

fn image_tag_around(html: &str, source: &str) -> Option<(usize, usize)> {
    let at = html.find(&format!("\"{source}\""))?;
    let start = html[..at].rfind("<img")?;
    let end = html[at..].find('>')? + at + 1;
    Some((start, end))
}

/// The `files` property Teams writes for a shared OneDrive or SharePoint file.
fn files_property(files: &[FileReference]) -> Value {
    Value::Array(
        files
            .iter()
            .map(|file| {
                let file_type = file
                    .name
                    .rsplit_once('.')
                    .map(|(_, extension)| extension.to_ascii_lowercase())
                    .unwrap_or_default();
                json!({
                    "@type": FILE_ITEMTYPE,
                    "version": 2,
                    "id": file.attachment_id,
                    "itemid": file.attachment_id,
                    "baseUrl": site_url(&file.content_url),
                    "type": file_type,
                    "fileType": file_type,
                    "title": file.name,
                    "fileName": file.name,
                    "state": "active",
                    "objectUrl": file.content_url,
                    "fileInfo": {
                        "itemId": null,
                        "fileUrl": file.content_url,
                        "siteUrl": site_url(&file.content_url),
                        "serverRelativeUrl": "",
                        "shareUrl": null,
                        "shareId": null,
                    },
                    "fileChicletState": {"serviceName": "p2p", "state": "active"},
                    "permissionScope": "users",
                })
            })
            .collect(),
    )
}

/// `https://tenant-my.sharepoint.com/personal/ann/` for a file under that site.
fn site_url(url: &str) -> String {
    SITE_SEGMENTS
        .iter()
        .find_map(|segment| {
            let start = url.find(segment)? + segment.len();
            let end = url[start..].find('/')? + start + 1;
            Some(url[..end].to_owned())
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uploaded(object_id: &str) -> AmsImage {
        AmsImage {
            object_id: object_id.into(),
            url: format!("https://ams.example/{object_id}"),
        }
    }

    #[test]
    fn hosted_images_become_ams_images_in_place_or_last() {
        let html = r#"<p>a<img src="../hostedContents/2/$value" width="20">b</p>"#;
        let body = with_ams_images(html, &[uploaded("one"), uploaded("two")]);
        assert_eq!(
            body,
            r#"<p>a<img itemtype="http://schema.skype.com/AMSImage" itemscope="" src="https://ams.example/two" itemid="two" alt="image">b</p><p><img itemtype="http://schema.skype.com/AMSImage" itemscope="" src="https://ams.example/one" itemid="one" alt="image"></p>"#
        );
    }

    #[test]
    fn a_file_names_its_site() {
        let files = files_property(&[FileReference {
            attachment_id: "guid-1".into(),
            content_url:
                "https://contoso-my.sharepoint.com/personal/ann_contoso_com/Documents/Plan.DOCX"
                    .into(),
            name: "Plan.DOCX".into(),
        }]);
        assert_eq!(
            files[0]["baseUrl"],
            "https://contoso-my.sharepoint.com/personal/ann_contoso_com/"
        );
        assert_eq!(files[0]["fileType"], "docx");
        assert_eq!(
            files[0]["fileInfo"]["fileUrl"],
            "https://contoso-my.sharepoint.com/personal/ann_contoso_com/Documents/Plan.DOCX"
        );
    }
}
