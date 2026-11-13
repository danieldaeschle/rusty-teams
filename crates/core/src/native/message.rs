use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, Utc};
use graph::Message;
use serde_json::{Value, json};

use super::markup::graph_body;
use crate::forwarded::FORWARDED_CONTENT_TYPE;
use chatsvc::graph_events::{event_detail, is_event, user_id};

const BOT_PREFIX: &str = "28:";
const FORWARD_CONTEXT: &str = "originalMessageContext";
const ANNOTATION_KEYS: [&str; 3] = ["follow", "star", "acks"];
const SWIFT_MARKER: &str = "<Swift b64=\"";

/// A chat service message as Graph would serve it, so the record mapping stays the same for both sources.
pub fn graph_message(conversation_id: &str, native: &Value) -> Option<Message> {
    serde_json::from_value(graph_message_json(conversation_id, native)?).ok()
}

pub(crate) fn graph_message_json(conversation_id: &str, native: &Value) -> Option<Value> {
    let message_type = text(native, &["messagetype", "messageType"])?;
    let id = chatsvc::id_text(native.get("id")?)?;
    let properties = &native["properties"];
    let created = time(
        native,
        &[
            "originalarrivaltime",
            "originalArrivalTime",
            "composetime",
            "composeTime",
        ],
    )?;
    let content = text(native, &["content"]).unwrap_or_default();
    let sender = sender(native);
    let sender_user_id = sender
        .pointer("/user/id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let mut message = json!({
        "id": id,
        "createdDateTime": created,
        "lastModifiedDateTime": millis(&native["version"]).map(|time| time.to_rfc3339()).unwrap_or_else(|| created.clone()),
        "lastEditedDateTime": millis(&properties["edittime"]).map(|time| time.to_rfc3339()),
        "deletedDateTime": millis(&properties["deletetime"]).map(|time| time.to_rfc3339()),
        "from": sender,
        "importance": text(properties, &["importance"]),
        "subject": text(properties, &["subject"]),
        "reactions": reactions(native),
    });
    if is_event(message_type) {
        message["messageType"] = json!("systemEventMessage");
        message["eventDetail"] = event_detail(
            message_type,
            content,
            conversation_id,
            sender_user_id.as_deref(),
        )?;
        message["from"] = Value::Null;
        message["body"] = json!({"contentType": "html", "content": ""});
        return Some(message);
    }
    if !is_user_message(message_type) {
        return None;
    }
    message["messageType"] = json!("message");
    let forwards = forward_contexts(properties);
    let forward_ids: Vec<String> = forwards.iter().map(|context| context.id.clone()).collect();
    let body = graph_body(&content.replace("\r\n", "\n"), &forward_ids);
    let mut attachments = file_attachments(properties);
    attachments.extend(card_attachments(properties));
    if message_type == "RichText/Media_Card" {
        attachments.extend(swift_attachments(content));
    }
    attachments.extend(body.quotes.iter().map(|quote| {
        json!({
            "id": quote.message_id,
            "contentType": "messageReference",
            "content": json!({
                "messageId": quote.message_id,
                "messagePreview": quote.preview,
                "messageSender": quote.sender_mri.as_deref().map(|mri| json!({"user": {"id": user_id(mri), "displayName": quote.sender_name}})),
            }).to_string(),
        })
    }));
    attachments.extend(
        forwards
            .iter()
            .zip(&body.forwards)
            .map(|(context, inner)| context.attachment(inner)),
    );
    let mut html = if message_type == "RichText/Media_Card" {
        String::new()
    } else {
        body.html
    };
    for marker in &attachments {
        let Some(attachment_id) = marker["id"].as_str() else {
            continue;
        };
        let tag = format!("<attachment id=\"{attachment_id}\"></attachment>");
        if !html.contains(&tag) {
            html.push_str(&tag);
        }
    }
    message["body"] = json!({
        "contentType": if message_type == "Text" { "text" } else { "html" },
        "content": html,
    });
    message["mentions"] = mentions(properties, &body.mention_texts);
    message["attachments"] = Value::Array(attachments);
    Some(message)
}

fn is_user_message(message_type: &str) -> bool {
    message_type == "Text" || message_type.starts_with("RichText")
}

fn sender(native: &Value) -> Value {
    let Some(mri) = text(native, &["from"]).and_then(|from| from.rsplit('/').next()) else {
        return Value::Null;
    };
    let name = text(native, &["imdisplayname", "imDisplayName"])
        .map(str::trim)
        .filter(|name| !name.is_empty());
    if let Some(bot_id) = mri.strip_prefix(BOT_PREFIX) {
        return json!({"application": {"id": bot_id, "displayName": name}});
    }
    if mri.starts_with("8:") {
        return json!({"user": {"id": user_id(mri), "displayName": name}});
    }
    Value::Null
}

fn reactions(native: &Value) -> Value {
    let emotions = chatsvc::parse_message_emotions(&json!({"messages": [native]}));
    Value::Array(
        emotions
            .into_iter()
            .flat_map(|message| message.reactions)
            .filter(|reaction| !ANNOTATION_KEYS.contains(&reaction.key.as_str()))
            .map(|reaction| {
                json!({
                    "reactionType": chatsvc::reaction_glyph_for_key(&reaction.key).unwrap_or(reaction.key),
                    "createdDateTime": reaction.time_ms.and_then(DateTime::<Utc>::from_timestamp_millis).map(|time| time.to_rfc3339()),
                    "user": {"user": {"id": reaction.user_id, "displayName": null}},
                })
            })
            .collect(),
    )
}

fn mentions(properties: &Value, texts: &std::collections::HashMap<String, String>) -> Value {
    Value::Array(
        json_list(&properties["mentions"])
            .iter()
            .filter_map(|mention| {
                let item_id = chatsvc::id_text(mention.get("itemid")?)?;
                let mri = mention["mri"].as_str().unwrap_or_default();
                let name = mention["displayName"].as_str().map(str::to_owned);
                let identity = json!({"id": user_id(mri), "displayName": name});
                let mentioned = match mention["mentionType"].as_str().unwrap_or("person") {
                    _ if mri.starts_with("19:") => json!({"conversation": {"id": mri, "displayName": name, "conversationIdentityType": "chat"}}),
                    "tag" => json!({"tag": identity}),
                    "channel" | "team" => json!({"conversation": identity}),
                    _ if mri.starts_with(BOT_PREFIX) => json!({"application": {"id": &mri[BOT_PREFIX.len()..], "displayName": name}}),
                    _ => json!({"user": identity}),
                };
                Some(json!({
                    "id": item_id.parse::<i64>().ok()?,
                    "mentionText": texts.get(&item_id).cloned().or(name),
                    "mentioned": mentioned,
                }))
            })
            .collect(),
    )
}

fn file_attachments(properties: &Value) -> Vec<Value> {
    json_list(&properties["files"])
        .iter()
        .filter_map(|file| {
            let id = text(file, &["itemid", "id"])?;
            let url = file
                .pointer("/fileInfo/fileUrl")
                .and_then(Value::as_str)
                .or_else(|| text(file, &["objectUrl"]))?;
            Some(json!({
                "id": id,
                "contentType": "reference",
                "contentUrl": url,
                "name": text(file, &["fileName", "title"]),
            }))
        })
        .collect()
}

fn card_attachments(properties: &Value) -> Vec<Value> {
    json_list(&properties["cards"])
        .iter()
        .filter_map(|card| {
            let content = match &card["content"] {
                Value::String(text) => text.clone(),
                Value::Null => return None,
                other => other.to_string(),
            };
            Some(json!({
                "id": text(card, &["cardClientId"]),
                "contentType": text(card, &["contentType"]).unwrap_or("application/vnd.microsoft.card.adaptive"),
                "content": content,
                "teamsAppId": text(card, &["appId"]),
            }))
        })
        .collect()
}

fn swift_attachments(content: &str) -> Vec<Value> {
    let Some(start) = content
        .find(SWIFT_MARKER)
        .map(|start| start + SWIFT_MARKER.len())
    else {
        return Vec::new();
    };
    let Some(encoded) = content[start..].split('"').next() else {
        return Vec::new();
    };
    let Some(swift) = STANDARD
        .decode(encoded)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
    else {
        return Vec::new();
    };
    swift["attachments"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(index, attachment)| {
            let content = match &attachment["content"] {
                Value::String(text) => text.clone(),
                Value::Null => return None,
                other => other.to_string(),
            };
            Some(json!({
                "id": format!("swift-{index}"),
                "contentType": text(attachment, &["contentType"]),
                "content": content,
            }))
        })
        .collect()
}

struct ForwardContext {
    id: String,
    conversation_id: Option<String>,
    sender_id: Option<String>,
    sent_at: Option<String>,
}

impl ForwardContext {
    fn attachment(&self, inner_html: &str) -> Value {
        json!({
            "id": self.id,
            "contentType": FORWARDED_CONTENT_TYPE,
            "content": json!({
                "originalMessageId": self.id,
                "originalMessageContent": inner_html,
                "originalConversationId": self.conversation_id,
                "originalSentDateTime": self.sent_at,
                "originalMessageSender": self.sender_id.as_ref().map(|id| json!({"user": {"id": id, "displayName": null}})),
            }).to_string(),
        })
    }
}

fn forward_contexts(properties: &Value) -> Vec<ForwardContext> {
    (0..)
        .map(|index| match index {
            0 => FORWARD_CONTEXT.to_owned(),
            index => format!("{FORWARD_CONTEXT}{index}"),
        })
        .map_while(|key| {
            properties
                .get(&key)
                .filter(|context| context.is_object())
                .cloned()
        })
        .filter_map(|context| {
            Some(ForwardContext {
                id: chatsvc::id_text(context.get("messageId")?)?,
                conversation_id: text(&context, &["originalThreadId"]).map(str::to_owned),
                sender_id: text(&context, &["originalSender"]).map(|mri| user_id(mri).to_owned()),
                sent_at: text(&context, &["originalSentTime"]).map(str::to_owned),
            })
        })
        .collect()
}

/// Chat service properties carry lists either as JSON text or as arrays.
fn json_list(value: &Value) -> Vec<Value> {
    match value {
        Value::Array(items) => items.clone(),
        Value::String(text) => serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|parsed| parsed.as_array().cloned())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn text<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value.get(*key)?.as_str())
        .filter(|text| !text.is_empty())
}

fn time(value: &Value, keys: &[&str]) -> Option<String> {
    let text = text(value, keys)?;
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|time| time.with_timezone(&Utc).to_rfc3339())
}

fn millis(value: &Value) -> Option<DateTime<Utc>> {
    let millis = value.as_i64().or_else(|| value.as_str()?.parse().ok())?;
    (millis > 0)
        .then(|| DateTime::<Utc>::from_timestamp_millis(millis))
        .flatten()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::mapping::message_record;

    const CHAT: &str = "19:chat@thread.v2";

    fn native(message_type: &str, content: &str, properties: Value) -> Value {
        json!({
            "id": "1794474789021",
            "messagetype": message_type,
            "content": content,
            "from": "https://teams.cloud.microsoft/api/chatsvc/emea/v1/users/ME/contacts/8:orgid:ann",
            "imdisplayname": "Ann Example",
            "originalarrivaltime": "2026-11-12T09:13:09.0210000Z",
            "composetime": "2026-11-12T09:13:09.0210000Z",
            "version": "1794474789021",
            "properties": properties,
        })
    }

    #[test]
    fn a_rich_text_message_maps_like_graph() {
        let message = native(
            "RichText/Html",
            r#"<p>hi <span itemtype="http://schema.skype.com/Mention" itemscope="" itemid="0">Bo</span></p>"#,
            json!({
                "mentions": r#"[{"@type":"http://schema.skype.com/Mention","itemid":0,"mri":"8:orgid:bob","mentionType":"person","displayName":"Bo Sample"}]"#,
                "emotions": [{"key": "like", "users": [{"mri": "8:orgid:bob", "time": 1794474799021i64, "value": "1"}]}],
                "edittime": 1794474799021i64,
                "importance": "high",
            }),
        );
        let graph = graph_message(CHAT, &message).unwrap();
        assert_eq!(graph.from.as_ref().unwrap().user_id(), Some("ann"));
        assert_eq!(
            graph.from.as_ref().unwrap().display_name(),
            Some("Ann Example")
        );
        assert_eq!(graph.mentions[0].mention_text.as_deref(), Some("Bo"));
        assert_eq!(graph.reactions[0].reaction_type, "👍");
        let record = message_record(CHAT, &graph).unwrap();
        assert_eq!(record.message_id, "1794474789021");
        assert_eq!(record.body_html, r#"<p>hi <at id="0">Bo</at></p>"#);
        assert!(record.edited_at.is_some());
        assert!(record.mentions_json.contains("\"user_id\":\"bob\""));
        assert_eq!(record.importance, store::MessageImportance::High);
    }

    #[test]
    fn files_and_cards_become_attachments() {
        let message = native(
            "RichText/Html",
            r#"<p>see</p><span itemid="c1" itemscope="" itemtype="http://schema.skype.com/InputExtension"><span itemprop="cardId"></span></span>"#,
            json!({
                "files": r#"[{"itemid":"f1","fileName":"plan.docx","fileInfo":{"fileUrl":"https://contoso.sharepoint.com/plan.docx"}}]"#,
                "cards": [{"cardClientId": "c1", "appId": "app", "contentType": "application/vnd.microsoft.card.adaptive", "content": {"type": "AdaptiveCard", "body": []}}],
            }),
        );
        let graph = graph_message(CHAT, &message).unwrap();
        let kinds: Vec<_> = graph
            .attachments
            .iter()
            .map(|attachment| attachment.content_type.clone().unwrap())
            .collect();
        assert_eq!(
            kinds,
            vec!["reference", "application/vnd.microsoft.card.adaptive"]
        );
        assert_eq!(graph.attachments[0].name.as_deref(), Some("plan.docx"));
        let html = graph.body.unwrap().content.unwrap();
        assert_eq!(
            html,
            r#"<p>see</p><attachment id="c1"></attachment><attachment id="f1"></attachment>"#
        );
    }

    #[test]
    fn a_forward_carries_the_original() {
        let message = native(
            "RichText/Html",
            r#"<blockquote itemtype="http://schema.skype.com/Forward"><p>old</p></blockquote>"#,
            json!({"originalMessageContext": {"messageId": 1793274845211i64, "originalSender": "8:orgid:bob", "originalSentTime": "2026-10-29T11:54:05.211Z", "originalThreadId": "19:other@thread.v2"}}),
        );
        let graph = graph_message(CHAT, &message).unwrap();
        let forward = &graph.attachments[0];
        assert_eq!(
            forward.content_type.as_deref(),
            Some(FORWARDED_CONTENT_TYPE)
        );
        let content: Value = serde_json::from_str(forward.content.as_deref().unwrap()).unwrap();
        assert_eq!(content["originalMessageId"], "1793274845211");
        assert_eq!(content["originalMessageContent"], "<p>old</p>");
        assert_eq!(content["originalMessageSender"]["user"]["id"], "bob");
    }

    #[test]
    fn a_deleted_message_is_deleted() {
        let graph = graph_message(
            CHAT,
            &native("RichText/Html", "", json!({"deletetime": 1794474799021i64})),
        )
        .unwrap();
        assert!(graph.is_deleted());
    }

    #[test]
    fn a_bot_card_is_decoded() {
        let swift = STANDARD.encode(r#"{"type":"message/card","attachments":[{"contentType":"application/vnd.microsoft.card.adaptive","content":{"type":"AdaptiveCard"}}]}"#);
        let mut message = native(
            "RichText/Media_Card",
            &format!(r#"<URIObject type="SWIFT.1"><Swift b64="{swift}"/></URIObject>"#),
            json!({}),
        );
        message["from"] = json!("https://x/contacts/28:bot-1");
        let graph = graph_message(CHAT, &message).unwrap();
        assert_eq!(graph.from.as_ref().unwrap().application_id(), Some("bot-1"));
        assert_eq!(graph.attachments.len(), 1);
        assert!(
            graph.attachments[0]
                .content
                .as_deref()
                .unwrap()
                .contains("AdaptiveCard")
        );
    }

    #[test]
    fn controls_are_skipped_and_events_kept() {
        assert!(graph_message(CHAT, &native("Control/Typing", "", json!({}))).is_none());
        let call = native(
            "Event/Call",
            "<ended/><partlist count=\"0\"></partlist><callId>c</callId>",
            json!({}),
        );
        let graph = graph_message(CHAT, &call).unwrap();
        assert_eq!(graph.message_type.as_deref(), Some("systemEventMessage"));
        assert!(
            message_record(CHAT, &graph)
                .unwrap()
                .call_event_json
                .is_some()
        );
    }
}
