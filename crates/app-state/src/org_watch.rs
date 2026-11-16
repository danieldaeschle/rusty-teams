//! Keeps an org that is not open in the window live for notifications, like Teams does for other orgs:
//! its own realtime socket, and a toast for every new chat message from someone else.
use std::collections::HashMap;
use std::time::Duration;

use app_base::orgs::Org;
use chatsvc::{ChatListing, Conversations, EventKind, Realtime, RealtimeConfig, RealtimeEvent};
use serde_json::Value;
use session::Session;
use tokio::sync::mpsc;

use crate::backend::BackendEvent;

const RETRY: Duration = Duration::from_secs(30);
const PREVIEW_CHARS: usize = 200;
/// Chats only, like the default Teams settings for other orgs; channel posts and system threads stay quiet.
const CHANNEL_SUFFIX: &str = "@thread.tacv2";
const SYSTEM_PREFIX: &str = "48:";
const MESSAGE_TYPES: [&str; 2] = ["RichText/Html", "Text"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrgMessage {
    pub org: Org,
    pub conversation_id: String,
    pub message_id: String,
    pub chat_title: String,
    pub sender_name: String,
    pub preview: String,
    pub direct: bool,
}

pub(crate) async fn watch(
    session: Session,
    org: Org,
    me: Option<String>,
    events: mpsc::UnboundedSender<BackendEvent>,
) {
    let listing = ChatListing::new(&session);
    let conversations = Conversations::new(&session);
    let mut muted: HashMap<String, bool> = HashMap::new();
    loop {
        match Realtime::start_with(&session, RealtimeConfig::default()).await {
            Ok(mut realtime) => {
                while let Some(event) = realtime.recv().await {
                    let RealtimeEvent::Message(event) = event else {
                        continue;
                    };
                    let (EventKind::NewMessage, Some(conversation_id), Some(message_id)) =
                        (event.kind, event.conversation_id, event.message_id)
                    else {
                        continue;
                    };
                    if !is_chat(&conversation_id) {
                        continue;
                    }
                    if !muted.contains_key(&conversation_id)
                        && let Ok(states) = conversations.conversation_states().await
                    {
                        muted.extend(states.into_iter().map(|state| (state.id, state.muted)));
                    }
                    if muted.get(&conversation_id).copied().unwrap_or(false) {
                        continue;
                    }
                    let Ok(message) = listing.message(&conversation_id, &message_id).await else {
                        continue;
                    };
                    let conversation = listing.conversation(&conversation_id).await.ok();
                    let parsed = org_message(
                        &org,
                        me.as_deref(),
                        &conversation_id,
                        &message_id,
                        &message,
                        conversation.as_ref(),
                    );
                    if let Some(parsed) = parsed
                        && events.send(BackendEvent::OrgMessage(parsed)).is_err()
                    {
                        return;
                    }
                }
            }
            Err(error) => crate::diagnostics::record(format!(
                "orgs: live updates for another org failed: {error}"
            )),
        }
        tokio::time::sleep(RETRY).await;
    }
}

fn is_chat(conversation_id: &str) -> bool {
    !conversation_id.starts_with(SYSTEM_PREFIX) && !conversation_id.ends_with(CHANNEL_SUFFIX)
}

fn org_message(
    org: &Org,
    me: Option<&str>,
    conversation_id: &str,
    message_id: &str,
    message: &Value,
    conversation: Option<&Value>,
) -> Option<OrgMessage> {
    let message_type = message["messagetype"].as_str()?;
    if !MESSAGE_TYPES.contains(&message_type) {
        return None;
    }
    let from = message["from"]
        .as_str()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if me.is_some_and(|me| from.ends_with(me)) {
        return None;
    }
    let sender_name = message["imdisplayname"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let text = store::plain_text(message["content"].as_str().unwrap_or_default());
    let preview: String = text.trim().chars().take(PREVIEW_CHARS).collect();
    if preview.is_empty() {
        return None;
    }
    let properties = conversation.map(|conversation| &conversation["threadProperties"]);
    let topic = properties
        .and_then(|properties| properties["topic"].as_str())
        .map(str::trim)
        .filter(|topic| !topic.is_empty());
    let direct = properties
        .and_then(|properties| properties["uniquerosterthread"].as_str())
        .is_some_and(|flag| flag == "true")
        || conversation_id.ends_with("@unq.gbl.spaces");
    Some(OrgMessage {
        org: org.clone(),
        conversation_id: conversation_id.to_owned(),
        message_id: message_id.to_owned(),
        chat_title: topic.unwrap_or(&sender_name).to_owned(),
        sender_name,
        preview,
        direct,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn org() -> Org {
        Org {
            id: "bbbb".into(),
            name: "Guest Org".into(),
            guest: true,
        }
    }

    fn message(from: &str, kind: &str, content: &str) -> Value {
        json!({"messagetype": kind, "from": format!("https://x/v1/users/ME/contacts/8:orgid:{from}"), "imdisplayname": "Ann Example", "content": content})
    }

    #[test]
    fn a_chat_message_from_someone_else_becomes_a_toast_with_the_topic() {
        let parsed = org_message(
            &org(),
            Some("me-1"),
            "19:abc@thread.v2",
            "1",
            &message("ann-2", "RichText/Html", "<p>Hello <b>there</b></p>"),
            Some(&json!({"threadProperties": {"topic": "Release"}})),
        )
        .unwrap();
        assert_eq!(parsed.chat_title, "Release");
        assert_eq!(parsed.sender_name, "Ann Example");
        assert!(parsed.preview.starts_with("Hello"));
        assert!(!parsed.direct);
    }

    #[test]
    fn own_messages_control_messages_and_channels_stay_quiet() {
        let own = message("ME-1", "RichText/Html", "hi");
        assert_eq!(
            org_message(&org(), Some("me-1"), "19:a@thread.v2", "1", &own, None),
            None
        );
        let control = message("ann-2", "ThreadActivity/AddMember", "x");
        assert_eq!(
            org_message(&org(), Some("me-1"), "19:a@thread.v2", "1", &control, None),
            None
        );
        assert!(!is_chat("19:general@thread.tacv2"));
        assert!(!is_chat("48:notifications"));
        assert!(is_chat("19:a_b@unq.gbl.spaces"));
    }
}
