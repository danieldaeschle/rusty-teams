use serde_json::Value;
use session::{Request, Scope, Session};

use crate::error::{Error, Result};
use crate::messages::{MessageTransport, SessionMessageTransport, encode, ensure_success};
use crate::pins::{CSA_RESOURCE, CSA_SCOPE, CsaTransport, DEFAULT_REGION, SessionTransport};

const VIEW: &str = "view=msnp24Equivalent";
const NEWEST_ANCHOR_SLACK_MS: i64 = 60_000;

/// One page of the conversation list as the chat service sends it; `next_url` lists older conversations.
#[derive(Debug, Clone, PartialEq)]
pub struct ConversationPage {
    pub conversations: Vec<Value>,
    pub next_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MessagePage {
    pub messages: Vec<Value>,
    pub has_older: bool,
}

/// One chat with its members and meeting details, from the chat service aggregator.
pub async fn aggregator_chat(session: &Session, chat_id: &str) -> Result<Value> {
    let url = format!(
        "https://teams.cloud.microsoft/api/csa/{DEFAULT_REGION}/api/v1/teams/users/me/chats/{}",
        encode(chat_id)
    );
    let transport = SessionTransport::with_scope(session, Scope::new(CSA_RESOURCE, CSA_SCOPE));
    let answer = transport.send(Request::get(url)).await?;
    ensure_success(&answer)?;
    Ok(answer.body)
}

/// Raw reads of chats, threads and messages for the native chat path.
pub struct ChatListing<T: MessageTransport = SessionMessageTransport> {
    transport: T,
    region: String,
}

impl ChatListing<SessionMessageTransport> {
    pub fn new(session: &Session) -> Self {
        ChatListing::with_transport(SessionMessageTransport::new(session), DEFAULT_REGION)
    }
}

impl<T: MessageTransport> ChatListing<T> {
    pub fn with_transport(transport: T, region: &str) -> Self {
        ChatListing {
            transport,
            region: region.to_owned(),
        }
    }

    fn conversations_url(&self) -> String {
        format!(
            "https://teams.cloud.microsoft/api/chatsvc/{}/v1/users/ME/conversations",
            self.region
        )
    }

    pub fn first_page_url(&self, page_size: usize) -> String {
        format!("{}?{VIEW}&pageSize={page_size}", self.conversations_url())
    }

    pub async fn page(&self, url: &str) -> Result<ConversationPage> {
        let body = self.get(url).await?;
        let conversations = body
            .get("conversations")
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| Error::UnexpectedAnswer("no conversations list".into()))?;
        let next_url = body
            .pointer("/_metadata/backwardLink")
            .and_then(Value::as_str)
            .filter(|link| !link.is_empty() && *link != url && !conversations.is_empty())
            .map(str::to_owned);
        Ok(ConversationPage {
            conversations,
            next_url,
        })
    }

    pub async fn conversation(&self, conversation_id: &str) -> Result<Value> {
        let url = format!(
            "{}/{}?{VIEW}",
            self.conversations_url(),
            encode(conversation_id)
        );
        self.get(&url).await
    }

    /// The thread with its members; members carry ids and roles but no names.
    pub async fn thread(&self, thread_id: &str) -> Result<Value> {
        let url = format!(
            "https://teams.cloud.microsoft/api/chatsvc/{}/v1/threads/{}?{VIEW}",
            self.region,
            encode(thread_id)
        );
        self.get(&url).await
    }

    /// Newest arrival first; with `before_ms` only messages that arrived before it.
    /// The plain message list is ordered by last change, so an edit would move an old message into the newest page.
    pub async fn messages(
        &self,
        conversation_id: &str,
        before_ms: Option<i64>,
        page_size: usize,
    ) -> Result<MessagePage> {
        let base = format!(
            "{}/{}/messages",
            self.conversations_url(),
            encode(conversation_id)
        );
        let anchor_ms = before_ms.map_or_else(
            || chrono::Utc::now().timestamp_millis() + NEWEST_ANCHOR_SLACK_MS,
            |before_ms| before_ms - 1,
        );
        let url = format!(
            "{base}/epochTimeStamp/{anchor_ms}?pageSize={page_size}&direction=BACKWARD&includeMetadata=true&locationType=Primary"
        );
        let body = self.get(&url).await?;
        let messages: Vec<Value> = body
            .get("messages")
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| Error::UnexpectedAnswer("no messages list".into()))?
            .into_iter()
            .filter(|message| before_ms.is_none_or(|before| arrival_ms(message) < Some(before)))
            .collect();
        let has_older = body
            .pointer("/_metadata/backwardLink")
            .and_then(Value::as_str)
            .is_some_and(|link| !link.is_empty())
            && !messages.is_empty();
        Ok(MessagePage {
            messages,
            has_older,
        })
    }

    pub async fn message(&self, conversation_id: &str, message_id: &str) -> Result<Value> {
        let url = format!(
            "{}/{}/messages/{}?{VIEW}",
            self.conversations_url(),
            encode(conversation_id),
            encode(message_id)
        );
        self.get(&url).await
    }

    async fn get(&self, url: &str) -> Result<Value> {
        let answer = self.transport.send(Request::get(url)).await?;
        ensure_success(&answer)?;
        Ok(answer.body)
    }
}

fn arrival_ms(message: &Value) -> Option<i64> {
    crate::json_id::id_text(message.get("id")?)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use serde_json::json;
    use session::ApiResponse;

    use super::*;

    struct Canned {
        answers: Mutex<VecDeque<Value>>,
        requests: Mutex<Vec<String>>,
    }

    impl MessageTransport for &Canned {
        async fn send(&self, request: Request) -> Result<ApiResponse> {
            self.requests.lock().unwrap().push(request.url.clone());
            Ok(ApiResponse {
                status: 200,
                body: self.answers.lock().unwrap().pop_front().unwrap_or_default(),
                retry_after: None,
            })
        }
    }

    fn canned(answers: Vec<Value>) -> Canned {
        Canned {
            answers: Mutex::new(answers.into()),
            requests: Mutex::new(Vec::new()),
        }
    }

    #[tokio::test]
    async fn a_page_links_to_older_conversations() {
        let transport = canned(vec![json!({
            "conversations": [{"id": "19:a@thread.v2"}],
            "_metadata": {"backwardLink": "https://next"},
        })]);
        let listing = ChatListing::with_transport(&transport, "emea");
        let page = listing.page(&listing.first_page_url(50)).await.unwrap();
        assert_eq!(page.conversations.len(), 1);
        assert_eq!(page.next_url.as_deref(), Some("https://next"));
        assert_eq!(
            transport.requests.lock().unwrap()[0],
            "https://teams.cloud.microsoft/api/chatsvc/emea/v1/users/ME/conversations?view=msnp24Equivalent&pageSize=50"
        );
    }

    #[tokio::test]
    async fn older_messages_end_before_the_anchor() {
        let transport = canned(vec![json!({
            "messages": [{"id": "1000"}, {"id": "900"}],
            "_metadata": {"backwardLink": "https://older"},
        })]);
        let listing = ChatListing::with_transport(&transport, "emea");
        let page = listing
            .messages("19:a@thread.v2", Some(1000), 20)
            .await
            .unwrap();
        assert_eq!(page.messages, vec![json!({"id": "900"})]);
        assert!(page.has_older);
        assert!(transport.requests.lock().unwrap()[0].contains("/messages/epochTimeStamp/999?"));
    }
}
