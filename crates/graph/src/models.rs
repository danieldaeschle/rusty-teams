use chrono::{DateTime, Utc};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub display_name: Option<String>,
    pub mail: Option<String>,
    pub user_principal_name: Option<String>,
    pub job_title: Option<String>,
    pub department: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub user_id: Option<String>,
    pub tenant_id: Option<String>,
    pub display_name: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub id: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
pub struct Sender {
    pub user: Option<Identity>,
    pub application: Option<Identity>,
    pub tag: Option<Identity>,
    pub conversation: Option<Identity>,
}

impl Sender {
    pub fn display_name(&self) -> Option<&str> {
        [&self.user, &self.application, &self.tag, &self.conversation]
            .into_iter()
            .flatten()
            .find_map(|identity| identity.display_name.as_deref())
    }

    pub fn target_id(&self) -> Option<&str> {
        [&self.user, &self.application, &self.tag, &self.conversation]
            .into_iter()
            .flatten()
            .find_map(|identity| identity.id.as_deref())
    }

    pub fn user_id(&self) -> Option<&str> {
        self.user.as_ref().and_then(|user| user.id.as_deref())
    }

    pub fn application_id(&self) -> Option<&str> {
        self.application
            .as_ref()
            .and_then(|application| application.id.as_deref())
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Body {
    pub content_type: String,
    pub content: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub id: Option<String>,
    pub content_type: Option<String>,
    pub content_url: Option<String>,
    pub name: Option<String>,
    pub content: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Reaction {
    pub reaction_type: String,
    pub created_date_time: Option<DateTime<Utc>>,
    pub user: Option<Sender>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Mention {
    pub id: Option<i64>,
    pub mention_text: Option<String>,
    pub mentioned: Option<Sender>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub message_type: Option<String>,
    pub created_date_time: Option<DateTime<Utc>>,
    pub last_modified_date_time: Option<DateTime<Utc>>,
    pub last_edited_date_time: Option<DateTime<Utc>>,
    pub deleted_date_time: Option<DateTime<Utc>>,
    pub reply_to_id: Option<String>,
    pub subject: Option<String>,
    pub from: Option<Sender>,
    pub body: Option<Body>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default)]
    pub reactions: Vec<Reaction>,
    #[serde(default)]
    pub mentions: Vec<Mention>,
    #[serde(default)]
    pub replies: Vec<Message>,
}

impl Message {
    pub fn is_deleted(&self) -> bool {
        self.deleted_date_time.is_some()
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChatViewpoint {
    pub last_message_read_date_time: Option<DateTime<Utc>>,
    pub is_hidden: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Chat {
    pub id: String,
    pub topic: Option<String>,
    pub chat_type: String,
    pub last_updated_date_time: Option<DateTime<Utc>>,
    #[serde(default)]
    pub members: Vec<Member>,
    pub last_message_preview: Option<Message>,
    pub viewpoint: Option<ChatViewpoint>,
}

impl Chat {
    pub fn title(&self, my_user_id: &str) -> String {
        if let Some(topic) = self.topic.as_deref().filter(|topic| !topic.is_empty()) {
            return topic.to_owned();
        }
        let names: Vec<&str> = self
            .members
            .iter()
            .filter(|member| member.user_id.as_deref() != Some(my_user_id))
            .map(|member| {
                member
                    .display_name
                    .as_deref()
                    .or(member.email.as_deref())
                    .unwrap_or("?")
            })
            .collect();
        if names.is_empty() {
            self.preview_sender_name(my_user_id)
                .unwrap_or("(only you)")
                .to_owned()
        } else {
            names.join(", ")
        }
    }

    fn preview_sender_name(&self, my_user_id: &str) -> Option<&str> {
        let sender = self.last_message_preview.as_ref()?.from.as_ref()?;
        let application = sender
            .application
            .as_ref()
            .and_then(|identity| identity.display_name.as_deref());
        let other_user = sender
            .user
            .as_ref()
            .filter(|identity| identity.id.as_deref() != Some(my_user_id))
            .and_then(|identity| identity.display_name.as_deref());
        application.or(other_user).filter(|name| !name.is_empty())
    }

    pub fn last_message_time(&self) -> Option<DateTime<Utc>> {
        self.last_message_preview
            .as_ref()
            .and_then(|preview| preview.created_date_time)
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Team {
    pub id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Channel {
    pub id: String,
    pub display_name: String,
    pub membership_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Photo {
    pub bytes: Vec<u8>,
    pub content_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Presence {
    pub user_id: String,
    pub availability: String,
    pub activity: Option<String>,
}
