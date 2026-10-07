use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MentionTarget {
    User { user_id: String },
    Channel { channel_id: String },
    Team { team_id: String },
}

/// `id` matches the `<at id="n">` tag in the html body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingMention {
    pub id: u32,
    pub text: String,
    pub target: MentionTarget,
}

impl OutgoingMention {
    pub fn to_json(&self) -> Value {
        let mentioned = match &self.target {
            MentionTarget::User { user_id } => json!({
                "user": {"id": user_id, "displayName": self.text, "userIdentityType": "aadUser"}
            }),
            MentionTarget::Channel { channel_id } => json!({
                "conversation": {"id": channel_id, "displayName": self.text, "conversationIdentityType": "channel"}
            }),
            MentionTarget::Team { team_id } => json!({
                "conversation": {"id": team_id, "displayName": self.text, "conversationIdentityType": "team"}
            }),
        };
        json!({"id": self.id, "mentionText": self.text, "mentioned": mentioned})
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedImage {
    pub content_type: String,
    pub bytes: Arc<Vec<u8>>,
}

/// `attachment_id` is the GUID inside the file's eTag; it matches the `<attachment id>` tag in the html body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileReference {
    pub attachment_id: String,
    pub content_url: String,
    pub name: String,
}

/// A quote or card attachment of an edited message, sent back unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeptAttachment {
    pub id: String,
    pub content_type: Option<String>,
    pub content: Option<String>,
    pub content_url: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageExtras {
    pub images: Vec<HostedImage>,
    pub files: Vec<FileReference>,
    pub kept: Vec<KeptAttachment>,
}

impl MessageExtras {
    pub fn is_empty(&self) -> bool {
        self.images.is_empty() && self.files.is_empty() && self.kept.is_empty()
    }

    pub fn kept_html(&self) -> String {
        self.kept
            .iter()
            .map(|kept| format!("<attachment id=\"{}\"></attachment>", kept.id))
            .collect()
    }

    pub fn files_html(&self) -> String {
        self.files
            .iter()
            .map(|file| format!("<attachment id=\"{}\"></attachment>", file.attachment_id))
            .collect()
    }
}

fn extras_html(html: &str, extras: &MessageExtras) -> String {
    let mut content = extras.kept_html();
    content.push_str(html);
    for index in 1..=extras.images.len() {
        content.push_str(&format!(
            "<p><img src=\"../hostedContents/{index}/$value\"></p>"
        ));
    }
    content.push_str(&extras.files_html());
    content
}

pub(crate) fn message_body(
    html: &str,
    mentions: &[OutgoingMention],
    extras: &MessageExtras,
) -> Value {
    let mut body = json!({"body": {"contentType": "html", "content": extras_html(html, extras)}});
    if !mentions.is_empty() {
        body["mentions"] = Value::Array(mentions.iter().map(OutgoingMention::to_json).collect());
    }
    if !extras.images.is_empty() {
        body["hostedContents"] = Value::Array(
            extras
                .images
                .iter()
                .enumerate()
                .map(|(index, image)| {
                    json!({
                        "@microsoft.graph.temporaryId": (index + 1).to_string(),
                        "contentBytes": STANDARD.encode(image.bytes.as_slice()),
                        "contentType": image.content_type,
                    })
                })
                .collect(),
        );
    }
    if !extras.files.is_empty() || !extras.kept.is_empty() {
        let kept = extras.kept.iter().map(|kept| {
            json!({
                "id": kept.id,
                "contentType": kept.content_type,
                "content": kept.content,
                "contentUrl": kept.content_url,
                "name": kept.name,
            })
        });
        let files = extras.files.iter().map(|file| {
            json!({
                "id": file.attachment_id,
                "contentType": "reference",
                "contentUrl": file.content_url,
                "name": file.name,
            })
        });
        body["attachments"] = Value::Array(kept.chain(files).collect());
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mention(id: u32, text: &str, target: MentionTarget) -> OutgoingMention {
        OutgoingMention {
            id,
            text: text.to_owned(),
            target,
        }
    }

    #[test]
    fn user_mention_matches_the_documented_payload() {
        let value = mention(
            0,
            "Ada",
            MentionTarget::User {
                user_id: "u1".into(),
            },
        )
        .to_json();
        assert_eq!(
            value,
            json!({"id": 0, "mentionText": "Ada", "mentioned": {"user": {"id": "u1", "displayName": "Ada", "userIdentityType": "aadUser"}}})
        );
    }

    #[test]
    fn channel_and_team_mentions_use_conversation_identity() {
        let channel = mention(
            1,
            "General",
            MentionTarget::Channel {
                channel_id: "19:c".into(),
            },
        )
        .to_json();
        let team = mention(
            2,
            "Squad",
            MentionTarget::Team {
                team_id: "t".into(),
            },
        )
        .to_json();
        assert_eq!(
            channel["mentioned"]["conversation"]["conversationIdentityType"],
            "channel"
        );
        assert_eq!(
            team["mentioned"]["conversation"]["conversationIdentityType"],
            "team"
        );
        assert_eq!(team["mentioned"]["conversation"]["id"], "t");
    }

    #[test]
    fn body_without_mentions_has_no_mentions_key() {
        let none = MessageExtras::default();
        assert!(
            message_body("<p>x</p>", &[], &none)
                .get("mentions")
                .is_none()
        );
        let with = message_body(
            "<at id=\"0\">A</at>",
            &[mention(
                0,
                "A",
                MentionTarget::User {
                    user_id: "u".into(),
                },
            )],
            &none,
        );
        assert_eq!(with["mentions"].as_array().unwrap().len(), 1);
    }

    fn image(content_type: &str, bytes: &[u8]) -> HostedImage {
        HostedImage {
            content_type: content_type.to_owned(),
            bytes: Arc::new(bytes.to_vec()),
        }
    }

    #[test]
    fn plain_body_has_no_hosted_contents_or_attachments() {
        let body = message_body("<p>x</p>", &[], &MessageExtras::default());
        assert_eq!(
            body,
            json!({"body": {"contentType": "html", "content": "<p>x</p>"}})
        );
    }

    #[test]
    fn images_go_after_the_text_with_matching_temporary_ids() {
        let extras = MessageExtras {
            kept: Vec::new(),
            images: vec![image("image/png", &[1, 2, 3]), image("image/jpeg", &[4])],
            files: Vec::new(),
        };
        let body = message_body("<p>hi</p>", &[], &extras);
        assert_eq!(
            body["body"]["content"],
            "<p>hi</p><p><img src=\"../hostedContents/1/$value\"></p><p><img src=\"../hostedContents/2/$value\"></p>"
        );
        assert_eq!(
            body["hostedContents"],
            json!([
                {"@microsoft.graph.temporaryId": "1", "contentBytes": "AQID", "contentType": "image/png"},
                {"@microsoft.graph.temporaryId": "2", "contentBytes": "BA==", "contentType": "image/jpeg"},
            ])
        );
        assert!(body.get("attachments").is_none());
    }

    #[test]
    fn an_edit_keeps_the_original_files_and_image_elements() {
        let extras = MessageExtras {
            kept: Vec::new(),
            images: Vec::new(),
            files: vec![FileReference {
                attachment_id: "G1".into(),
                content_url: "https://x/a.pdf".into(),
                name: "a.pdf".into(),
            }],
        };
        let original_image = "<img src=\"https://graph.microsoft.com/v1.0/chats/c/messages/1/hostedContents/9/$value\">";
        let body = message_body(&format!("<p>new</p>{original_image}"), &[], &extras);
        assert_eq!(
            body["body"]["content"],
            format!("<p>new</p>{original_image}<attachment id=\"G1\"></attachment>")
        );
        assert_eq!(body["attachments"][0]["id"], "G1");
        assert!(body.get("hostedContents").is_none());
    }

    #[test]
    fn an_edit_sends_the_quote_back_unchanged_before_the_text() {
        let extras = MessageExtras {
            images: Vec::new(),
            files: Vec::new(),
            kept: vec![KeptAttachment {
                id: "Q1".into(),
                content_type: Some("messageReference".into()),
                content: Some("{\"messageId\":\"Q1\"}".into()),
                content_url: None,
                name: None,
            }],
        };
        let body = message_body("<p>new</p>", &[], &extras);
        assert_eq!(
            body["body"]["content"],
            "<attachment id=\"Q1\"></attachment><p>new</p>"
        );
        assert_eq!(body["attachments"][0]["contentType"], "messageReference");
        assert_eq!(body["attachments"][0]["content"], "{\"messageId\":\"Q1\"}");
    }

    #[test]
    fn files_become_reference_attachments_with_a_tag_in_the_html() {
        let extras = MessageExtras {
            kept: Vec::new(),
            images: Vec::new(),
            files: vec![FileReference {
                attachment_id: "AAAA-1111".into(),
                content_url: "https://contoso.sharepoint.com/Plan.docx".into(),
                name: "Plan.docx".into(),
            }],
        };
        let body = message_body("", &[], &extras);
        assert_eq!(
            body["body"]["content"],
            "<attachment id=\"AAAA-1111\"></attachment>"
        );
        assert_eq!(
            body["attachments"],
            json!([{
                "id": "AAAA-1111",
                "contentType": "reference",
                "contentUrl": "https://contoso.sharepoint.com/Plan.docx",
                "name": "Plan.docx",
            }])
        );
        assert!(body.get("hostedContents").is_none());
    }
}
