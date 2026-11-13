use chrono::{DateTime, Utc};
use serde_json::{Value, json};

const STARTED_TYPE: &str = "#microsoft.graph.callStartedEventMessageDetail";
const ENDED_TYPE: &str = "#microsoft.graph.callEndedEventMessageDetail";
const RECORDING_TYPE: &str = "#microsoft.graph.callRecordingEventMessageDetail";
const TRANSCRIPT_TYPE: &str = "#microsoft.graph.callTranscriptEventMessageDetail";
const MEMBERS_ADDED_TYPE: &str = "#microsoft.graph.membersAddedEventMessageDetail";
const MEMBERS_DELETED_TYPE: &str = "#microsoft.graph.membersDeletedEventMessageDetail";
const MEMBERS_LEFT_TYPE: &str = "#microsoft.graph.membersLeftEventMessageDetail";
const CHAT_RENAMED_TYPE: &str = "#microsoft.graph.chatRenamedEventMessageDetail";
const MEETING_CHAT_PREFIX: &str = "19:meeting_";
const ALL_HISTORY: &str = "0001-01-01T00:00:00Z";

/// The Graph `eventDetail` of a chat service system message; `None` for events Graph does not list.
pub fn event_detail(
    message_type: &str,
    content: &str,
    conversation_id: &str,
    sender_id: Option<&str>,
) -> Option<Value> {
    match message_type {
        "Event/Call" => call_detail(content, conversation_id, sender_id),
        "RichText/Media_CallRecording" => recording_detail(content),
        "RichText/Media_CallTranscript" => transcript_detail(content),
        "ThreadActivity/AddMember" => added_detail(content),
        "ThreadActivity/DeleteMember" => Some(json!({
            "@odata.type": MEMBERS_DELETED_TYPE,
            "members": targets(content),
            "initiator": user(tag_text(content, "initiator")),
        })),
        "ThreadActivity/MemberLeft" => left_detail(content),
        "ThreadActivity/TopicUpdate" if !conversation_id.starts_with(MEETING_CHAT_PREFIX) => {
            Some(json!({
                "@odata.type": CHAT_RENAMED_TYPE,
                "chatDisplayName": tag_text(content, "value").map(|topic| unescape(topic.trim())),
                "initiator": user(tag_text(content, "initiator")),
            }))
        }
        _ => None,
    }
}

/// The recording and call events of a chat service message page, as a Graph `{"value": [...]}` page.
pub fn graph_event_page(conversation_id: &str, chatsvc_page: &Value) -> Value {
    let events: Vec<Value> = chatsvc_page["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|message| {
            let message_type = message["messagetype"].as_str()?;
            if !is_event(message_type) {
                return None;
            }
            let sender_id = message["from"]
                .as_str()
                .map(|from| user_id(from.rsplit('/').next().unwrap_or(from)));
            let detail = event_detail(
                message_type,
                message["content"].as_str().unwrap_or_default(),
                conversation_id,
                sender_id,
            )?;
            let created = ["originalarrivaltime", "composetime"]
                .iter()
                .find_map(|key| message[*key].as_str())
                .and_then(|stamp| DateTime::parse_from_rfc3339(stamp).ok())
                .map(|stamp| stamp.with_timezone(&Utc).to_rfc3339());
            Some(json!({
                "id": crate::json_id::id_text(&message["id"])?,
                "createdDateTime": created,
                "eventDetail": detail,
            }))
        })
        .collect();
    json!({"value": events})
}

pub fn is_event(message_type: &str) -> bool {
    message_type == "Event/Call"
        || message_type.starts_with("ThreadActivity/")
        || message_type == "RichText/Media_CallRecording"
        || message_type == "RichText/Media_CallTranscript"
}

fn call_detail(content: &str, conversation_id: &str, sender_id: Option<&str>) -> Option<Value> {
    let kind = if has_flag(content, "ended") {
        ENDED_TYPE
    } else {
        STARTED_TYPE
    };
    let parts: Vec<&str> = content.split("<part ").skip(1).collect();
    let participants: Vec<Value> = parts
        .iter()
        .filter_map(|part| {
            let identity = attribute(&format!(" {part}"), "identity")?.to_owned();
            let name = tag_text(part, "displayName")
                .map(|name| unescape(name.trim()))
                .filter(|name| !name.is_empty());
            Some(json!({"participant": {"user": {"id": user_id(&identity), "displayName": name}}}))
        })
        .collect();
    let longest = parts
        .iter()
        .filter_map(|part| tag_text(part, "duration")?.trim().parse::<f64>().ok())
        .fold(None, |longest: Option<f64>, seconds| {
            Some(longest.map_or(seconds, |known| known.max(seconds)))
        });
    let event_type = tag_text(content, "callEventType").unwrap_or_default();
    let meeting = event_type.to_ascii_lowercase().contains("meeting")
        || conversation_id.starts_with(MEETING_CHAT_PREFIX)
        || tag_text(content, "meetingType").is_some_and(|kind| !kind.trim().is_empty());
    Some(json!({
        "@odata.type": kind,
        "callId": tag_text(content, "callId")?.trim(),
        "callDuration": longest.map(|seconds| format!("PT{}S", seconds.round() as u64)),
        "callEventType": if meeting { "meeting" } else { "call" },
        "callParticipants": participants,
        "initiator": user(sender_id),
    }))
}

fn recording_detail(content: &str) -> Option<Value> {
    let status = element_attribute(content, "RecordingStatus", "status")?;
    let status = match status.to_ascii_lowercase().as_str() {
        "started" | "initial" => "initial",
        "chunkfinished" => "chunkFinished",
        "success" => "success",
        _ => return None,
    };
    let name = element_attribute(content, "OriginalName", "v")
        .map(unescape)
        .or_else(|| tag_text(content, "Title").map(unescape));
    Some(json!({
        "@odata.type": RECORDING_TYPE,
        "callId": identifier(content, "callId")?,
        "callRecordingStatus": status,
        "callRecordingDisplayName": name,
        "callRecordingUrl": element_attribute(content, "a", "href").map(unescape),
        "callRecordingDuration": element_attribute(content, "RecordingContent", "duration").and_then(iso_duration),
        "meetingOrganizer": user(element_attribute(content, "MeetingOrganizerId", "value")),
        "initiator": user(element_attribute(content, "RecordingInitiatorId", "value")),
    }))
}

fn transcript_detail(content: &str) -> Option<Value> {
    let call_id =
        identifier(content, "callId").or_else(|| tag_text(content, "callId").map(str::trim))?;
    Some(json!({
        "@odata.type": TRANSCRIPT_TYPE,
        "callId": call_id,
        "meetingOrganizer": user(element_attribute(content, "MeetingOrganizerId", "value")),
    }))
}

fn added_detail(content: &str) -> Option<Value> {
    let members = targets(content);
    if members.as_array().is_none_or(Vec::is_empty) {
        return None;
    }
    let visible_from = tag_text(content, "sharehistorytime")
        .and_then(|time| time.trim().parse::<i64>().ok())
        .and_then(|millis| match millis {
            0 => Some(ALL_HISTORY.to_owned()),
            millis if millis > 0 => {
                DateTime::<Utc>::from_timestamp_millis(millis).map(|time| time.to_rfc3339())
            }
            _ => None,
        });
    Some(json!({
        "@odata.type": MEMBERS_ADDED_TYPE,
        "members": members,
        "initiator": user(tag_text(content, "initiator")),
        "visibleHistoryStartDateTime": visible_from,
    }))
}

fn left_detail(content: &str) -> Option<Value> {
    let detail: Value = serde_json::from_str(content).ok()?;
    let members: Vec<Value> = detail["members"]
        .as_array()?
        .iter()
        .filter_map(|member| {
            let id = member["id"].as_str()?;
            let name = member["friendlyname"]
                .as_str()
                .filter(|name| !name.is_empty());
            Some(json!({"id": user_id(id), "displayName": name}))
        })
        .collect();
    (!members.is_empty()).then(|| {
        json!({
            "@odata.type": MEMBERS_LEFT_TYPE,
            "members": members,
            "initiator": user(detail["initiator"].as_str()),
        })
    })
}

fn targets(content: &str) -> Value {
    Value::Array(
        tag_texts(content, "target")
            .into_iter()
            .map(str::trim)
            .filter(|target| target.starts_with("8:"))
            .map(|target| json!({"id": user_id(target), "displayName": null}))
            .collect(),
    )
}

fn user(mri: Option<&str>) -> Value {
    match mri.map(str::trim).filter(|mri| !mri.is_empty()) {
        Some(mri) => json!({"user": {"id": user_id(mri), "displayName": null}}),
        None => Value::Null,
    }
}

/// Graph names people and bots by the id after the last `:` of the MRI (`8:orgid:`, `28:`, `8:teamsvisitor:`).
pub fn user_id(mri: &str) -> &str {
    mri.rsplit(':').next().unwrap_or(mri)
}

fn has_flag(content: &str, name: &str) -> bool {
    content.contains(&format!("<{name}/>")) || content.contains(&format!("<{name} />"))
}

fn iso_duration(text: &str) -> Option<String> {
    let mut parts = text.split(':').rev();
    let seconds: f64 = parts.next()?.parse().ok()?;
    let minutes: u64 = parts.next().map_or(Some(0), |part| part.parse().ok())?;
    let hours: u64 = parts.next().map_or(Some(0), |part| part.parse().ok())?;
    Some(format!("PT{hours}H{minutes}M{}S", seconds.round() as u64))
}

pub fn tag_text<'a>(content: &'a str, tag: &str) -> Option<&'a str> {
    tag_texts(content, tag).into_iter().next()
}

fn tag_texts<'a>(content: &'a str, tag: &str) -> Vec<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut found = Vec::new();
    let mut rest = content;
    while let Some(start) = rest.find(&open) {
        let after = &rest[start + open.len()..];
        let Some(end) = after.find(&close) else {
            break;
        };
        found.push(&after[..end]);
        rest = &after[end + close.len()..];
    }
    found
}

fn element_attribute<'a>(content: &'a str, element: &str, name: &str) -> Option<&'a str> {
    let open = format!("<{element} ");
    let start = content.find(&open)?;
    let tag_end = content[start..].find('>')? + start;
    attribute(&content[start..tag_end], name)
}

fn identifier<'a>(content: &'a str, kind: &str) -> Option<&'a str> {
    let marker = format!("type=\"{kind}\"");
    let mut rest = content;
    while let Some(start) = rest.find("<Id ") {
        let tag_end = rest[start..].find('>')? + start;
        let tag = &rest[start..tag_end];
        if tag.contains(&marker) {
            return attribute(tag, "value").filter(|value| !value.is_empty());
        }
        rest = &rest[tag_end..];
    }
    None
}

pub fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut search = 0;
    while let Some(found) = tag[search..].find(name) {
        let start = search + found;
        let before = tag[..start].chars().last();
        let after = &tag[start + name.len()..];
        if before.is_some_and(char::is_whitespace)
            && let Some(value) = after.strip_prefix("=\"")
        {
            return value.find('"').map(|end| &value[..end]);
        }
        search = start + name.len();
    }
    None
}

pub fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_recording_page_reads_like_a_graph_page() {
        let content = r#"<URIObject type="Video.2/CallRecording.1"><RecordingStatus status="Success" code="200"></RecordingStatus><a href="https://contoso.sharepoint.com/:v:/rec">Play</a><OriginalName v="Daily.mp4" /><Identifiers><Id type="callId" value="call-1" /></Identifiers></URIObject>"#;
        let page = json!({"messages": [
            {"id": "1751975663649", "messagetype": "RichText/Media_CallRecording", "content": content, "originalarrivaltime": "2026-11-04T10:32:30.000Z", "from": "https://x/contacts/8:orgid:ann"},
            {"id": "1751975663650", "messagetype": "RichText/Html", "content": "<p>hi</p>"},
        ]});
        let graph = graph_event_page("19:a@thread.v2", &page);
        let events = graph["value"].as_array().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["id"], "1751975663649");
        assert_eq!(
            events[0]["eventDetail"]["callRecordingUrl"],
            "https://contoso.sharepoint.com/:v:/rec"
        );
        assert_eq!(events[0]["createdDateTime"], "2026-11-04T10:32:30+00:00");
    }

    #[test]
    fn pins_and_file_events_are_left_out() {
        assert!(
            event_detail(
                "ThreadActivity/PinnedItemsUpdate",
                "{}",
                "19:a@thread.v2",
                None
            )
            .is_none()
        );
        assert_eq!(json!(null), user(Some(" ")));
    }
}
