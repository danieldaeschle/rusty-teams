use serde_json::Value;
use session::{Request, Scope, Session};

use crate::error::{Error, Result};
use crate::messages::{MessageTransport, SessionMessageTransport, encode, ensure_success};
use crate::pins::{CSA_RESOURCE, CSA_SCOPE, CsaTransport, DEFAULT_REGION, SessionTransport};

const VIEW: &str = "view=msnp24Equivalent";
const NEWEST_ANCHOR_SLACK_MS: i64 = 60_000;
const MESSAGES_SEGMENT: &str = "messages";
const SYNC_CURSOR_PREFIX: &str = "syncState:";
const EPOCH_SEGMENT: &str = "epochTimeStamp";

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
    pub oldest_arrival_ms: Option<i64>,
    /// Where Teams says the older page is: `epochTimeStamp/<ms>?<query>`, for [`ChatListing::messages_at`].
    pub older_cursor: Option<String>,
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
            "{}/{}/{MESSAGES_SEGMENT}",
            self.conversations_url(),
            encode(conversation_id)
        );
        let anchor_ms = before_ms.map_or_else(
            || chrono::Utc::now().timestamp_millis() + NEWEST_ANCHOR_SLACK_MS,
            |before_ms| before_ms - 1,
        );
        let url = format!(
            "{base}/{EPOCH_SEGMENT}/{anchor_ms}?pageSize={page_size}&direction=BACKWARD&includeMetadata=true&locationType=Primary"
        );
        let body = self.get(&url).await?;
        message_page(&body, before_ms)
    }

    /// The older page a previous page's `older_cursor` points to. Only the anchor and query come
    /// from Teams: the host and conversation stay ours. The notes chat answered 404
    /// `LocationLookupFailed` to the anchor [`ChatListing::messages`] builds by hand.
    pub async fn messages_at(
        &self,
        conversation_id: &str,
        cursor: &str,
        before_ms: Option<i64>,
        page_size: usize,
    ) -> Result<MessagePage> {
        let url = if let Some(sync_state) = cursor.strip_prefix(SYNC_CURSOR_PREFIX) {
            if sync_state.is_empty() || sync_state.len() > 4_096 {
                return Err(Error::UnexpectedAnswer("invalid syncState cursor".into()));
            }
            format!(
                "{}/{}/{MESSAGES_SEGMENT}?{VIEW}&pageSize={page_size}&startTime={}&syncState={}",
                self.conversations_url(),
                encode(conversation_id),
                before_ms.unwrap_or(0),
                encode(sync_state)
            )
        } else if is_older_cursor(cursor) {
            format!(
                "{}/{}/{MESSAGES_SEGMENT}/{cursor}",
                self.conversations_url(),
                encode(conversation_id)
            )
        } else {
            return Err(Error::UnexpectedAnswer(format!(
                "not an older page cursor: {cursor}"
            )));
        };
        let body = self.get(&url).await?;
        message_page(&body, before_ms)
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

fn message_page(body: &Value, before_ms: Option<i64>) -> Result<MessagePage> {
    let messages: Vec<Value> = body
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| Error::UnexpectedAnswer("no messages list".into()))?
        .into_iter()
        .filter(|message| before_ms.is_none_or(|before| arrival_ms(message) < Some(before)))
        .collect();
    let backward_link = body
        .pointer("/_metadata/backwardLink")
        .and_then(Value::as_str)
        .filter(|link| !link.is_empty());
    let sync_link = body
        .pointer("/_metadata/syncState")
        .and_then(Value::as_str)
        .filter(|link| !link.is_empty());
    let has_older = !messages.is_empty() && (backward_link.is_some() || sync_link.is_some());
    let oldest_arrival_ms = messages.iter().filter_map(arrival_ms).min();
    Ok(MessagePage {
        older_cursor: if messages.is_empty() {
            None
        } else {
            backward_link
                .and_then(older_cursor)
                .or_else(|| backward_link.and_then(sync_state_of))
                .or_else(|| sync_link.and_then(sync_state_of))
        },
        has_older,
        messages,
        oldest_arrival_ms,
    })
}

/// `.../messages/epochTimeStamp/<ms>?<query>` becomes `epochTimeStamp/<ms>?<query>`; any other shape has none.
fn older_cursor(link: &str) -> Option<String> {
    let url = url::Url::parse(link).ok()?;
    let segments: Vec<&str> = url.path_segments()?.collect();
    let [.., messages, epoch, anchor] = segments.as_slice() else {
        return None;
    };
    if *messages != MESSAGES_SEGMENT || *epoch != EPOCH_SEGMENT || !is_digits(anchor) {
        return None;
    }
    Some(match url.query() {
        Some(query) => format!("{EPOCH_SEGMENT}/{anchor}?{query}"),
        None => format!("{EPOCH_SEGMENT}/{anchor}"),
    })
}

/// Consumer chats answer with `_metadata.syncState`, a `msgapi.teams.live.com` URL carrying the
/// cursor in its `syncState` query parameter. Keep only the cursor and use our own host later.
fn sync_state_of(link: &str) -> Option<String> {
    let url = url::Url::parse(link).ok()?;
    let state = url
        .query_pairs()
        .find(|(key, _)| key == "syncState")?
        .1
        .into_owned();
    (!state.is_empty()).then(|| format!("{SYNC_CURSOR_PREFIX}{state}"))
}

fn is_older_cursor(cursor: &str) -> bool {
    let (path, query) = cursor.split_once('?').unwrap_or((cursor, ""));
    path.strip_prefix(EPOCH_SEGMENT)
        .and_then(|rest| rest.strip_prefix('/'))
        .is_some_and(is_digits)
        && !query.contains(['#', '/', '\\'])
}

fn is_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
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

    const NOTES_BACKWARD_LINK: &str = "https://emea.ng.msg.teams.microsoft.com/v1/users/ME/conversations/48%3Anotes/messages/epochTimeStamp/1791368000009?pageSize=20&direction=BACKWARD&includeMetadata=true";

    #[tokio::test]
    async fn a_page_keeps_the_older_cursor_teams_links() {
        let transport = canned(vec![json!({
            "messages": [{"id": "1791368000010"}],
            "_metadata": {"backwardLink": NOTES_BACKWARD_LINK},
        })]);
        let listing = ChatListing::with_transport(&transport, "emea");
        let page = listing.messages("48:notes", None, 20).await.unwrap();
        assert_eq!(
            page.older_cursor.as_deref(),
            Some(
                "epochTimeStamp/1791368000009?pageSize=20&direction=BACKWARD&includeMetadata=true"
            )
        );
    }

    #[tokio::test]
    async fn the_older_page_follows_the_cursor_on_our_host() {
        let transport = canned(vec![json!({"messages": [{"id": "5"}], "_metadata": {}})]);
        let listing = ChatListing::with_transport(&transport, "emea");
        let page = listing
            .messages_at(
                "48:notes",
                &older_cursor(NOTES_BACKWARD_LINK).unwrap(),
                None,
                20,
            )
            .await
            .unwrap();
        assert_eq!(page.messages, vec![json!({"id": "5"})]);
        assert!(!page.has_older);
        assert_eq!(
            transport.requests.lock().unwrap()[0],
            "https://teams.cloud.microsoft/api/chatsvc/emea/v1/users/ME/conversations/48%3Anotes/messages/epochTimeStamp/1791368000009?pageSize=20&direction=BACKWARD&includeMetadata=true"
        );
    }

    #[test]
    fn links_of_another_shape_give_no_cursor() {
        for link in [
            "https://h/v1/users/ME/conversations/48%3Anotes/messages?startTime=1&syncState=x",
            "https://h/v1/users/ME/conversations/48%3Anotes/messages/epochTimeStamp/12a?x=1",
            "https://h/v1/users/ME/conversations/48%3Anotes/messages/epochTimeStamp/?x=1",
            "https://h/v1/users/ME/conversations/48%3Anotes/threads/epochTimeStamp/12",
            "https://h/messages/epochTimeStamp/12/..",
            "not a link",
        ] {
            assert_eq!(older_cursor(link), None, "{link}");
        }
    }

    #[tokio::test]
    async fn a_cursor_of_another_shape_is_refused_without_a_request() {
        let transport = canned(Vec::new());
        let listing = ChatListing::with_transport(&transport, "emea");
        for cursor in [
            "../../threads/x",
            "epochTimeStamp/12/../x",
            "epochTimeStamp/12?a=1#frag",
            "epochTimeStamp/12?a=/../x",
            "chatsvc:older",
        ] {
            assert!(
                matches!(
                    listing.messages_at("48:notes", cursor, None, 20).await,
                    Err(Error::UnexpectedAnswer(_))
                ),
                "{cursor}"
            );
        }
        assert!(transport.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_consumer_page_with_only_sync_state_keeps_the_cursor() {
        let transport = canned(vec![json!({
            "messages": [{"id": "1000"}],
            "_metadata": {"syncState": "https://msgapi.teams.live.com/v1/users/ME/conversations/19%3Aa/messages?syncState=abc%2F123%3D"},
        })]);
        let listing = ChatListing::with_transport(&transport, "emea");
        let page = listing.messages("19:a", None, 20).await.unwrap();
        assert!(page.has_older);
        assert_eq!(page.older_cursor.as_deref(), Some("syncState:abc/123="));
    }

    #[tokio::test]
    async fn the_consumer_older_page_uses_the_cursor_on_our_host() {
        let transport = canned(vec![json!({"messages": [{"id": "5"}], "_metadata": {}})]);
        let listing = ChatListing::with_transport(&transport, "emea");
        let page = listing
            .messages_at("48:notes", "syncState:abc/123=", Some(1000), 20)
            .await
            .unwrap();
        assert_eq!(page.messages, vec![json!({"id": "5"})]);
        assert_eq!(
            transport.requests.lock().unwrap()[0],
            "https://teams.cloud.microsoft/api/chatsvc/emea/v1/users/ME/conversations/48%3Anotes/messages?view=msnp24Equivalent&pageSize=20&startTime=1000&syncState=abc%2F123%3D"
        );
    }

    #[tokio::test]
    async fn a_malformed_sync_cursor_is_refused_without_a_request() {
        let transport = canned(Vec::new());
        let listing = ChatListing::with_transport(&transport, "emea");
        for cursor in ["syncState:", &format!("syncState:{}", "x".repeat(5_000))] {
            assert!(matches!(
                listing.messages_at("48:notes", cursor, None, 20).await,
                Err(Error::UnexpectedAnswer(_))
            ));
        }
        assert!(transport.requests.lock().unwrap().is_empty());
    }
}
