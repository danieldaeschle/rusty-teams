use std::time::Duration;

use chrono::Utc;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::{Value, json};
use session::{ApiResponse, IC3, Method, Request, Scope, Session};

use crate::error::{Error, Result};
use crate::pins::DEFAULT_REGION;

const MESSAGES_SCOPE: &str = "Teams.AccessAsUser.All";
pub fn messages_scope() -> Scope {
    Scope::new(IC3, MESSAGES_SCOPE)
}

pub const DEFAULT_RETRY_DELAY: Duration = Duration::from_millis(500);

const ENCODE_COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversationRef {
    Chat { chat_id: String },
    ChannelRoot { channel_id: String },
    ChannelReply { channel_id: String, root_id: String },
}

impl ConversationRef {
    pub fn chat(chat_id: &str) -> Self {
        ConversationRef::Chat {
            chat_id: chat_id.to_owned(),
        }
    }

    pub fn channel_root(channel_id: &str) -> Self {
        ConversationRef::ChannelRoot {
            channel_id: channel_id.to_owned(),
        }
    }

    pub fn channel_reply(channel_id: &str, root_id: &str) -> Self {
        ConversationRef::ChannelReply {
            channel_id: channel_id.to_owned(),
            root_id: root_id.to_owned(),
        }
    }

    pub fn conversation_id(&self) -> String {
        match self {
            ConversationRef::Chat { chat_id } => chat_id.clone(),
            ConversationRef::ChannelRoot { channel_id } => channel_id.clone(),
            ConversationRef::ChannelReply {
                channel_id,
                root_id,
            } => format!("{channel_id};messageid={root_id}"),
        }
    }
}

pub(crate) fn encode(value: &str) -> String {
    utf8_percent_encode(value, ENCODE_COMPONENT).to_string()
}

pub trait MessageTransport {
    fn send(&self, request: Request) -> impl std::future::Future<Output = Result<ApiResponse>>;
}

pub struct SessionMessageTransport {
    session: Session,
    scope: Scope,
}

impl SessionMessageTransport {
    pub fn new(session: &Session) -> Self {
        SessionMessageTransport {
            session: session.clone(),
            scope: messages_scope(),
        }
    }
}

impl MessageTransport for SessionMessageTransport {
    async fn send(&self, request: Request) -> Result<ApiResponse> {
        let mut answers = self.session.batch(&[request], &self.scope).await?;
        Ok(answers.remove(0))
    }
}

pub struct Messages<T: MessageTransport = SessionMessageTransport> {
    pub(crate) transport: T,
    pub(crate) base_url: String,
    retry_delay: Duration,
}

impl Messages<SessionMessageTransport> {
    pub fn new(session: &Session) -> Self {
        Self::with_region(session, DEFAULT_REGION)
    }

    pub fn with_region(session: &Session, region: &str) -> Self {
        Messages::with_transport(SessionMessageTransport::new(session), region)
    }
}

impl<T: MessageTransport> Messages<T> {
    pub fn with_transport(transport: T, region: &str) -> Self {
        Messages {
            transport,
            base_url: format!(
                "https://teams.cloud.microsoft/api/chatsvc/{region}/v1/users/ME/conversations"
            ),
            retry_delay: DEFAULT_RETRY_DELAY,
        }
    }

    pub fn with_retry_delay(mut self, retry_delay: Duration) -> Self {
        self.retry_delay = retry_delay;
        self
    }

    pub fn message_url(&self, conversation: &ConversationRef, message_id: &str) -> String {
        format!(
            "{}/{}/messages/{}",
            self.base_url,
            encode(&conversation.conversation_id()),
            encode(message_id)
        )
    }

    pub async fn edit_message(
        &self,
        conversation: &ConversationRef,
        message_id: &str,
        html: &str,
    ) -> Result<()> {
        let url = self.message_url(conversation, message_id);
        let body = edit_body(message_id, html);
        let mut answer = self.put(&url, &body).await?;
        if answer.status == 404 {
            tokio::time::sleep(self.retry_delay).await;
            answer = self.put(&url, &body).await?;
        }
        ensure_success(&answer)
    }

    pub async fn soft_delete_message(
        &self,
        conversation: &ConversationRef,
        message_id: &str,
    ) -> Result<()> {
        let url = format!(
            "{}?behavior=softDelete",
            self.message_url(conversation, message_id)
        );
        let answer = self.transport.send(Request::delete(&url)).await?;
        ensure_success(&answer)
    }

    pub async fn set_emotion(
        &self,
        conversation: &ConversationRef,
        message_id: &str,
        key: &str,
    ) -> Result<()> {
        let url = self.emotions_url(conversation, message_id);
        let body = json!({"emotions": {"key": key, "value": Utc::now().timestamp_millis()}});
        self.send_retrying_server_errors(Method::Put, &url, &body)
            .await
    }

    pub async fn unset_emotion(
        &self,
        conversation: &ConversationRef,
        message_id: &str,
        key: &str,
    ) -> Result<()> {
        let url = self.emotions_url(conversation, message_id);
        let body = json!({"emotions": {"key": key}});
        self.send_retrying_server_errors(Method::Delete, &url, &body)
            .await
    }

    pub async fn unset_emotions(
        &self,
        conversation: &ConversationRef,
        message_id: &str,
        keys: &[&str],
    ) -> Result<()> {
        let mut last_error = None;
        let mut any_succeeded = false;
        for key in keys {
            match self.unset_emotion(conversation, message_id, key).await {
                Ok(()) => any_succeeded = true,
                Err(error) => last_error = Some(error),
            }
        }
        match last_error {
            Some(error) if !any_succeeded => Err(error),
            _ => Ok(()),
        }
    }

    pub async fn send_typing(&self, conversation: &ConversationRef, active: bool) -> Result<()> {
        let url = format!(
            "{}/{}/messages",
            self.base_url,
            encode(&conversation.conversation_id())
        );
        let answer = self
            .transport
            .send(Request::with_body(Method::Post, &url, typing_body(active)))
            .await?;
        ensure_success(&answer)
    }

    fn emotions_url(&self, conversation: &ConversationRef, message_id: &str) -> String {
        format!(
            "{}/properties?name=emotions",
            self.message_url(conversation, message_id)
        )
    }

    async fn send_retrying_server_errors(
        &self,
        method: Method,
        url: &str,
        body: &Value,
    ) -> Result<()> {
        let request = || Request::with_body(method, url, body.clone());
        let mut answer = self.transport.send(request()).await?;
        if (500..600).contains(&answer.status) {
            tokio::time::sleep(self.retry_delay).await;
            answer = self.transport.send(request()).await?;
        }
        ensure_success(&answer)
    }

    async fn put(&self, url: &str, body: &Value) -> Result<ApiResponse> {
        self.transport
            .send(Request::with_body(Method::Put, url, body.clone()))
            .await
    }
}

fn edit_body(message_id: &str, html: &str) -> Value {
    json!({
        "id": message_id,
        "content": html,
        "messagetype": "RichText/Html",
        "contenttype": "text",
        "properties": {"edittime": Utc::now().timestamp_millis().to_string()},
    })
}

fn typing_body(active: bool) -> Value {
    json!({
        "content": "",
        "messagetype": if active { "Control/Typing" } else { "Control/ClearTyping" },
        "contenttype": "Application/Message",
    })
}

pub(crate) fn ensure_success(answer: &ApiResponse) -> Result<()> {
    if answer.is_success() {
        Ok(())
    } else {
        Err(Error::Session(session::Error::api(
            answer.status,
            "chatsvc",
            answer.body.clone(),
        )))
    }
}
