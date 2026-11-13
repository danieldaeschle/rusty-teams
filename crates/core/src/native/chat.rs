use chrono::{DateTime, Utc};
use graph::Chat;
use serde_json::{Value, json};

use super::message::graph_message_json;

const ORGID_PREFIX: &str = "8:orgid:";
const ONE_ON_ONE_SUFFIX: &str = "@unq.gbl.spaces";
const OWNER_ROLE: &str = "admin";

/// A chat member before its name is known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeMember {
    pub user_id: String,
    pub owner: bool,
    pub name: Option<String>,
}

/// Graph lists chats, meetings and 1:1s; channels, teams and the service streams stay out.
pub fn chat_type(conversation: &Value) -> Option<&'static str> {
    let id = conversation["id"].as_str()?;
    if id.ends_with(ONE_ON_ONE_SUFFIX) {
        return Some("oneOnOne");
    }
    let thread = &conversation["threadProperties"];
    match thread["productThreadType"].as_str() {
        Some("OneToOneChat") => return Some("oneOnOne"),
        Some("Meeting") => return Some("meeting"),
        Some("Chat") => return Some("group"),
        Some(_) => return None,
        None => {}
    }
    match thread["threadType"].as_str() {
        Some("chat") if id.starts_with("19:") => Some("group"),
        Some("meeting") => Some("meeting"),
        _ => None,
    }
}

/// The two user ids of a `19:{a}_{b}@unq.gbl.spaces` chat.
pub fn one_on_one_ids(chat_id: &str) -> Option<[String; 2]> {
    let pair = chat_id
        .strip_prefix("19:")?
        .strip_suffix(ONE_ON_ONE_SUFFIX)?;
    let (first, second) = pair.split_once('_')?;
    Some([first.to_owned(), second.to_owned()])
}

pub fn thread_members(thread: &Value) -> Vec<NativeMember> {
    thread["members"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|member| {
            let user_id = member["id"].as_str()?.strip_prefix(ORGID_PREFIX)?;
            Some(NativeMember {
                user_id: user_id.to_owned(),
                owner: member["role"]
                    .as_str()
                    .is_some_and(|role| role.eq_ignore_ascii_case(OWNER_ROLE)),
                name: None,
            })
        })
        .collect()
}

pub fn graph_member(member: &NativeMember, email: Option<&str>) -> Value {
    json!({
        "id": member.user_id,
        "userId": member.user_id,
        "displayName": member.name,
        "email": email,
        "roles": if member.owner { vec!["owner"] } else { Vec::new() },
    })
}

/// A chat service conversation in the shape of a Graph chat with `members` and `lastMessagePreview`.
pub fn graph_chat(conversation: &Value, members: Vec<Value>) -> Option<Chat> {
    let id = conversation["id"].as_str()?;
    let kind = chat_type(conversation)?;
    let thread = &conversation["threadProperties"];
    let created = millis_text(&thread["createdat"]);
    let read = chatsvc::ReadHorizon::from_conversation(conversation);
    let preview = preview(id, &conversation["lastMessage"]);
    let last_updated = preview
        .as_ref()
        .and_then(|preview| preview["createdDateTime"].as_str().map(str::to_owned))
        .or_else(|| created.clone());
    serde_json::from_value(json!({
        "id": id,
        "topic": thread["topic"].as_str().filter(|topic| !topic.trim().is_empty()),
        "chatType": kind,
        "createdDateTime": created,
        "lastUpdatedDateTime": last_updated,
        "members": members,
        "lastMessagePreview": preview,
        "viewpoint": {
            "lastMessageReadDateTime": read_time(read.consumed_until_ms),
            "isHidden": null,
        },
    }))
    .ok()
}

/// One chat from the chat service aggregator (`teams/users/me/chats/{id}`), which also has the meeting link.
pub fn graph_chat_from_aggregator(chat: &Value, members: Vec<Value>) -> Option<Chat> {
    let id = chat["id"].as_str()?;
    let kind = if chat["isOneOnOne"].as_bool() == Some(true) || id.ends_with(ONE_ON_ONE_SUFFIX) {
        "oneOnOne"
    } else if chat["chatType"].as_str() == Some("meeting") {
        "meeting"
    } else {
        "group"
    };
    let preview = preview(id, &chat["lastMessage"]);
    let read_ms = chat
        .pointer("/consumptionHorizon/originalArrivalTime")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    serde_json::from_value(json!({
        "id": id,
        "topic": chat["title"].as_str().filter(|topic| !topic.trim().is_empty() && kind != "oneOnOne"),
        "chatType": kind,
        "createdDateTime": chat["createdAt"].as_str().and_then(rfc3339),
        "lastUpdatedDateTime": preview.as_ref().and_then(|preview| preview["createdDateTime"].as_str().map(str::to_owned)),
        "members": members,
        "lastMessagePreview": preview,
        "viewpoint": {"lastMessageReadDateTime": read_time(read_ms), "isHidden": null},
        "onlineMeetingInfo": {"joinWebUrl": chat.pointer("/meetingInformation/meetingJoinUrl")},
    }))
    .ok()
}

pub fn aggregator_members(chat: &Value) -> Vec<NativeMember> {
    chat["members"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|member| {
            let user_id = member["mri"].as_str()?.strip_prefix(ORGID_PREFIX)?;
            Some(NativeMember {
                user_id: user_id.to_owned(),
                owner: member["role"]
                    .as_str()
                    .is_some_and(|role| role.eq_ignore_ascii_case(OWNER_ROLE)),
                name: member["friendlyName"]
                    .as_str()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned),
            })
        })
        .collect()
}

/// Graph previews every last message; one the record mapping skips still dates the chat.
fn preview(conversation_id: &str, last_message: &Value) -> Option<Value> {
    if last_message
        .as_object()
        .is_none_or(|object| object.is_empty())
    {
        return None;
    }
    graph_message_json(conversation_id, last_message).or_else(|| {
        let id = chatsvc::id_text(last_message.get("id")?)?;
        let time = [
            "originalarrivaltime",
            "originalArrivalTime",
            "composetime",
            "composeTime",
        ]
        .iter()
        .find_map(|key| last_message[*key].as_str())
        .and_then(rfc3339)?;
        Some(json!({"id": id, "messageType": "systemEventMessage", "createdDateTime": time}))
    })
}

fn read_time(millis: i64) -> Option<String> {
    (millis > 0)
        .then(|| DateTime::<Utc>::from_timestamp_millis(millis))
        .flatten()
        .map(|time| time.to_rfc3339())
}

fn millis_text(value: &Value) -> Option<String> {
    let millis = value.as_i64().or_else(|| value.as_str()?.parse().ok())?;
    read_time(millis)
}

fn rfc3339(text: &str) -> Option<String> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|time| time.with_timezone(&Utc).to_rfc3339())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::mapping::chat_record;

    fn conversation(id: &str, product: &str) -> Value {
        json!({
            "id": id,
            "properties": {"consumptionhorizon": "1794474789021;1794474789021;1"},
            "threadProperties": {"productThreadType": product, "topic": "Plan", "createdat": "1790000000000"},
            "lastMessage": {
                "id": "1794474789021",
                "messagetype": "RichText/Html",
                "content": "<p>hi</p>",
                "from": "https://x/contacts/8:orgid:ann",
                "imdisplayname": "Ann",
                "originalarrivaltime": "2026-11-12T09:13:09.021Z",
            },
        })
    }

    #[test]
    fn channels_and_streams_are_not_chats() {
        assert_eq!(
            chat_type(&conversation("19:a@thread.tacv2", "TeamsStandardChannel")),
            None
        );
        assert_eq!(
            chat_type(&conversation("48:notifications", "StreamOfNotifications")),
            None
        );
        assert_eq!(
            chat_type(&conversation("19:a@thread.v2", "Meeting")),
            Some("meeting")
        );
        assert_eq!(
            chat_type(&json!({"id": "19:ann_bob@unq.gbl.spaces"})),
            Some("oneOnOne")
        );
    }

    #[test]
    fn a_read_group_chat_maps_to_a_record() {
        let members = vec![
            graph_member(
                &NativeMember {
                    user_id: "me".into(),
                    owner: true,
                    name: Some("Me".into()),
                },
                None,
            ),
            graph_member(
                &NativeMember {
                    user_id: "ann".into(),
                    owner: false,
                    name: Some("Ann Example".into()),
                },
                None,
            ),
        ];
        let chat = graph_chat(&conversation("19:a@thread.v2", "Chat"), members).unwrap();
        let record = chat_record(&chat, "me");
        assert_eq!(record.title, "Plan");
        assert_eq!(record.kind, "group");
        assert_eq!(record.member_summary, "Ann Example");
        assert_eq!(record.last_message_preview.as_deref(), Some("hi"));
        assert!(!record.unread);
        assert!(chat.members[0].is_owner());
    }

    #[test]
    fn an_unmapped_last_message_still_dates_the_chat() {
        let mut pinned = conversation("19:a@thread.v2", "Chat");
        pinned["lastMessage"]["messagetype"] = json!("ThreadActivity/PinnedItemsUpdate");
        let chat = graph_chat(&pinned, Vec::new()).unwrap();
        assert!(chat.last_message_time().is_some());
    }

    #[test]
    fn the_aggregator_chat_has_the_meeting_link() {
        let chat = json!({
            "id": "19:meeting_x@thread.v2",
            "chatType": "meeting",
            "title": "Daily",
            "members": [{"mri": "8:orgid:ann", "role": "Admin", "friendlyName": "Ann"}],
            "meetingInformation": {"meetingJoinUrl": "https://teams.microsoft.com/l/meetup-join/x"},
            "lastMessage": {"id": "1", "messageType": "Text", "content": "hi", "from": "8:orgid:ann", "originalArrivalTime": "2026-11-12T09:13:09.021Z"},
        });
        let members: Vec<Value> = aggregator_members(&chat)
            .iter()
            .map(|member| graph_member(member, None))
            .collect();
        let graph = graph_chat_from_aggregator(&chat, members).unwrap();
        assert_eq!(graph.chat_type, "meeting");
        assert_eq!(graph.members[0].display_name.as_deref(), Some("Ann"));
        assert_eq!(
            graph.online_meeting_info.unwrap().join_web_url.as_deref(),
            Some("https://teams.microsoft.com/l/meetup-join/x")
        );
    }

    #[test]
    fn one_on_one_ids_split_the_pair() {
        assert_eq!(
            one_on_one_ids("19:ann_bob@unq.gbl.spaces"),
            Some(["ann".to_owned(), "bob".to_owned()])
        );
        assert_eq!(one_on_one_ids("19:a@thread.v2"), None);
    }
}
