//! Reads chat data from the same Teams service used by the signed-in web client.
//! Graph remains preferred when its Chat.Read scope is available.
use std::sync::atomic::Ordering;

use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use session::{IC3, Scope};

use crate::urls::segment;
use crate::{Chat, Error, Graph, Message, Page, Result};

const BASE: &str = "https://teams.cloud.microsoft/api/chatsvc/emea/v1/users/ME/conversations";
const TEAMS_PREFIX: &str = "https://teams.cloud.microsoft/api/chatsvc/";
const GRAPH_CHAT: u8 = 1;
const TEAMS_CHAT: u8 = 2;

fn scope() -> Scope {
    Scope::new(IC3, "Teams.AccessAsUser.All")
}

pub(crate) fn chats_url(top: usize) -> String {
    format!(
        "{BASE}?view=msnp24Equivalent&pageSize={}",
        top.clamp(1, 200)
    )
}

pub(crate) fn is_chat_url(url: &str) -> bool {
    url.strip_prefix(TEAMS_PREFIX).is_some_and(|tail| {
        tail.split('/')
            .next()
            .is_some_and(|region| !region.is_empty())
            && tail.contains("/v1/users/ME/conversations")
    })
}

fn next_link(body: &Value) -> Option<String> {
    body.pointer("/_metadata/backwardLink")
        .and_then(Value::as_str)
        .filter(|link| is_chat_url(link))
        .map(str::to_owned)
}

impl Graph {
    pub(crate) async fn use_teams_chat(&self) -> bool {
        match self.chat_transport.load(Ordering::Relaxed) {
            GRAPH_CHAT => return false,
            TEAMS_CHAT => return true,
            _ => {}
        }
        if self
            .session()
            .has_access_token(&Scope::graph("Chat.Read"))
            .await
        {
            self.chat_transport.store(GRAPH_CHAT, Ordering::Relaxed);
            false
        } else if self.session().has_access_token(&scope()).await {
            self.chat_transport.store(TEAMS_CHAT, Ordering::Relaxed);
            true
        } else {
            // Preserve the usual authentication diagnosis if neither service is usable.
            false
        }
    }

    async fn teams_body(&self, url: &str) -> Result<Value> {
        if !is_chat_url(url) {
            return Err(Error::ForeignNextLink);
        }
        Ok(self
            .session()
            .request(session::Method::Get, url, &scope(), None)
            .await?
            .body)
    }

    pub(crate) async fn teams_chats_page(&self, url: &str) -> Result<Page<Chat>> {
        let body = self.teams_body(url).await?;
        let mut items = conversations(&body)?;
        // ChatMember.Read is granted to the Teams web client even when Chat.Read is not.
        // Batch member lookups so unnamed 1:1 chats keep their real titles.
        for chunk in items.chunks_mut(crate::BATCH_SIZE) {
            let requests: Vec<_> = chunk
                .iter()
                .map(|chat| session::Request::get(crate::urls::chat_members(&chat.id)))
                .collect();
            if let Ok(answers) = self
                .session()
                .batch(&requests, &Scope::graph("ChatMember.Read"))
                .await
            {
                for (chat, answer) in chunk.iter_mut().zip(answers) {
                    if answer.is_success() {
                        if let Some(members) = answer.body.get("value") {
                            chat.members =
                                serde_json::from_value(members.clone()).unwrap_or_default();
                        }
                    }
                }
            }
        }
        Ok(Page {
            items,
            next_link: next_link(&body).filter(|link| link != url),
            delta_link: None,
            scope: scope(),
        })
    }

    pub(crate) async fn teams_chat(&self, id: &str) -> Result<Chat> {
        let body = self
            .teams_body(&format!("{BASE}/{}?view=msnp24Equivalent", segment(id)))
            .await?;
        let mut chat = conversation(&body)
            .ok_or_else(|| Error::Decode("Teams conversation is not a chat".into()))?;
        chat.members = self.chat_members(id).await.unwrap_or_default();
        Ok(chat)
    }

    pub(crate) async fn teams_messages(
        &self,
        id: &str,
        before: Option<DateTime<Utc>>,
        top: usize,
    ) -> Result<Page<Message>> {
        let collection = format!("{BASE}/{}/messages", segment(id));
        let url = match before {
            Some(before) => format!(
                "{collection}/epochTimeStamp/{}?direction=BACKWARD&pageSize={}&includeMetadata=true&locationType=Primary",
                before.timestamp_millis(),
                top.clamp(1, 200)
            ),
            None => format!(
                "{collection}?pageSize={}&view=msnp24Equivalent",
                top.clamp(1, 200)
            ),
        };
        let body = self.teams_body(&url).await?;
        let items = body
            .get("messages")
            .and_then(Value::as_array)
            .ok_or_else(|| Error::Decode("Teams response has no messages list".into()))?
            .iter()
            .filter_map(message)
            .filter(|m| before.is_none_or(|b| m.created_date_time.is_none_or(|t| t < b)))
            .collect();
        Ok(Page {
            items,
            next_link: next_link(&body),
            delta_link: None,
            scope: scope(),
        })
    }

    pub(crate) async fn teams_message(&self, chat_id: &str, message_id: &str) -> Result<Message> {
        let body = self
            .teams_body(&format!(
                "{BASE}/{}/messages/{}",
                segment(chat_id),
                segment(message_id)
            ))
            .await?;
        message(&body).ok_or_else(|| Error::Decode("Teams response has no message id".into()))
    }
}

fn text_id(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_u64().map(|n| n.to_string()))
}

fn date(value: &Value) -> Option<DateTime<Utc>> {
    value
        .as_str()
        .and_then(|s| s.parse::<DateTime<Utc>>().ok())
        .or_else(|| {
            value
                .as_i64()
                .or_else(|| value.as_str()?.parse().ok())
                .and_then(DateTime::from_timestamp_millis)
        })
}

fn conversation(value: &Value) -> Option<Chat> {
    let id = value.get("id")?.as_str()?;
    let props = &value["threadProperties"];
    let kind = props["threadType"].as_str().unwrap_or("chat");
    if !matches!(kind, "chat" | "meeting") || id == "48:notes" {
        return None;
    }
    let chat_type = if kind == "meeting" {
        "meeting"
    } else if id.ends_with("@unq.gbl.spaces") {
        "oneOnOne"
    } else {
        "group"
    };
    let preview = message(&value["lastMessage"]);
    let last_time = preview.as_ref().and_then(|m| m.created_date_time);
    let hidden = [&value["properties"]["hidden"], &props["hidden"]]
        .iter()
        .any(|v| v.as_bool() == Some(true) || v.as_str() == Some("true"));
    serde_json::from_value(json!({
        "id": id, "chatType": chat_type, "topic": props["topic"],
        "createdDateTime": date(&props["createdat"]), "lastUpdatedDateTime": last_time,
        "viewpoint": {"isHidden": hidden}, "members": []
    }))
    .ok()
    .map(|mut chat: Chat| {
        chat.last_message_preview = preview;
        chat
    })
}

fn conversations(body: &Value) -> Result<Vec<Chat>> {
    let entries = body
        .get("conversations")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Decode("Teams response has no conversations list".into()))?;
    Ok(entries.iter().filter_map(conversation).collect())
}

fn message(value: &Value) -> Option<Message> {
    let id = text_id(value.get("id")?)?;
    let kind = value["messagetype"].as_str().unwrap_or("Text");
    let from = value["from"]
        .as_str()
        .unwrap_or_default()
        .rsplit('/')
        .next()
        .unwrap_or_default();
    let sender = if let Some(id) = from.strip_prefix("8:orgid:") {
        json!({"user":{"id":id,"displayName":value["imdisplayname"]}})
    } else if let Some(id) = from.strip_prefix("28:") {
        json!({"application":{"id":id,"displayName":value["imdisplayname"]}})
    } else {
        json!({"user":{"id":from,"displayName":value["imdisplayname"]}})
    };
    let created = date(&value["originalarrivaltime"])
        .or_else(|| date(&value["composetime"]))
        .or_else(|| date(&json!(id)));
    let deleted = value["properties"]["isdeleted"].as_bool() == Some(true)
        || value["properties"]["isdeleted"].as_str() == Some("true");
    serde_json::from_value(json!({
        "id":id, "messageType": if kind.starts_with("ThreadActivity/") {"systemEventMessage"} else {"message"},
        "createdDateTime":created, "lastModifiedDateTime":date(&value["version"]).or(created),
        "lastEditedDateTime":date(&value["properties"]["edittime"]), "isDeleted":deleted,
        "from":sender, "body":{"contentType":if kind == "Text" {"text"} else {"html"}, "content":value["content"]},
        "subject":value["properties"]["subject"], "importance":value["properties"]["importance"]
    })).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_fallback_excludes_channels_and_preserves_preview_and_kind() {
        let body = json!({"conversations":[
            {"id":"19:a_b@unq.gbl.spaces","threadProperties":{"threadType":"chat"},"lastMessage":{
                "id":"1794520000000","messagetype":"Text","content":"hello","from":"x/contacts/8:orgid:b","imdisplayname":"Bea","originalarrivaltime":"2026-11-12T20:00:00Z"}},
            {"id":"19:channel@thread.skype","threadProperties":{"threadType":"topic"}},
            {"id":"19:meeting_a@thread.v2","threadProperties":{"threadType":"meeting","topic":"Planning"}}
        ]});
        let chats = conversations(&body).unwrap();
        assert_eq!(chats.len(), 2);
        assert_eq!(chats[0].chat_type, "oneOnOne");
        let preview = chats[0].last_message_preview.as_ref().unwrap();
        assert_eq!(preview.from.as_ref().unwrap().user_id(), Some("b"));
        assert_eq!(
            preview.body.as_ref().unwrap().content.as_deref(),
            Some("hello")
        );
        assert_eq!(chats[1].chat_type, "meeting");
    }

    #[test]
    fn continuation_cannot_send_credentials_to_another_origin() {
        assert!(is_chat_url(&chats_url(50)));
        for url in [
            "https://evil.example/api/chatsvc/emea/v1/users/ME/conversations",
            "https://teams.cloud.microsoft.evil.example/api/chatsvc/emea/v1/users/ME/conversations",
            "http://teams.cloud.microsoft/api/chatsvc/emea/v1/users/ME/conversations",
        ] {
            assert!(next_link(&json!({"_metadata":{"backwardLink":url}})).is_none());
        }
        assert!(conversations(&json!({"error":"denied"})).is_err());
    }
}
