use std::cmp::Reverse;
use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde_json::Value;
use session::Request;

use crate::error::{Error, Result};
use crate::messages::{MessageTransport, Messages, encode, ensure_success};

const CALL_LOG_CONVERSATION: &str = "48:calllogs";
const LIST_QUERY: &str = "view=msnp24Equivalent&pageSize=200";
const MULTI_PARTY: &str = "multiParty";
const VOICEMAIL_TYPE: &str = "voicemail";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallDirection {
    Incoming,
    Outgoing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallOutcome {
    Accepted,
    Missed,
    Declined,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallLogEntry {
    pub call_id: String,
    pub started_at: DateTime<Utc>,
    pub duration_seconds: Option<i64>,
    pub direction: CallDirection,
    pub outcome: CallOutcome,
    pub is_meeting: bool,
    pub peer_id: Option<String>,
    pub peer_name: Option<String>,
    pub thread_id: Option<String>,
}

impl CallLogEntry {
    pub fn missed(&self) -> bool {
        self.direction == CallDirection::Incoming && self.outcome == CallOutcome::Missed
    }
}

fn text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)?
        .as_str()
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn time(value: &Value, key: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value.get(key)?.as_str()?)
        .ok()
        .map(|time| time.with_timezone(&Utc))
}

fn log_of(message: &Value) -> Option<Value> {
    match message.pointer("/properties/call-log")? {
        Value::String(text) => serde_json::from_str(text).ok(),
        object @ Value::Object(_) => Some(object.clone()),
        _ => None,
    }
}

fn direction_of(log: &Value) -> Option<CallDirection> {
    match log.get("callDirection")?.as_str()? {
        "incoming" => Some(CallDirection::Incoming),
        "outgoing" => Some(CallDirection::Outgoing),
        _ => None,
    }
}

fn outcome_of(log: &Value, connected: bool) -> CallOutcome {
    match log.get("callState").and_then(Value::as_str) {
        Some("accepted") => CallOutcome::Accepted,
        Some("declined") => CallOutcome::Declined,
        Some("missed") => CallOutcome::Missed,
        _ if connected => CallOutcome::Accepted,
        _ => CallOutcome::Missed,
    }
}

fn peer_of(log: &Value, direction: CallDirection) -> (Option<String>, Option<String>) {
    let (participant_key, id_key) = match direction {
        CallDirection::Incoming => ("originatorParticipant", "originator"),
        CallDirection::Outgoing => ("targetParticipant", "target"),
    };
    let participant = log.get(participant_key).unwrap_or(&Value::Null);
    if participant.get("type").and_then(Value::as_str) == Some(VOICEMAIL_TYPE) {
        return (None, None);
    }
    let peer_id = text(participant, "id").or_else(|| text(log, id_key));
    (peer_id, text(participant, "displayName"))
}

fn call_log_entry(message: &Value) -> Option<CallLogEntry> {
    let log = log_of(message)?;
    let direction = direction_of(&log)?;
    let started_at = time(&log, "startTime")?;
    let connected_at = time(&log, "connectTime");
    let duration_seconds = connected_at
        .zip(time(&log, "endTime"))
        .map(|(connected, ended)| (ended - connected).num_seconds().max(0));
    let (peer_id, peer_name) = peer_of(&log, direction);
    Some(CallLogEntry {
        call_id: text(&log, "callId").or_else(|| text(message, "id"))?,
        started_at,
        duration_seconds,
        direction,
        outcome: outcome_of(&log, connected_at.is_some()),
        is_meeting: log.get("callType").and_then(Value::as_str) == Some(MULTI_PARTY),
        peer_id,
        peer_name,
        thread_id: text(&log, "threadId"),
    })
}

pub fn parse_call_logs(body: &Value) -> Result<Vec<CallLogEntry>> {
    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::UnexpectedAnswer("no messages list".into()))?;
    let mut seen = HashSet::new();
    let mut entries: Vec<CallLogEntry> = messages
        .iter()
        .filter_map(call_log_entry)
        .filter(|entry| seen.insert(entry.call_id.clone()))
        .collect();
    entries.sort_by_key(|entry| Reverse(entry.started_at));
    Ok(entries)
}

impl<T: MessageTransport> Messages<T> {
    pub fn call_logs_url(&self) -> String {
        format!(
            "{}/{}/messages?{LIST_QUERY}",
            self.base_url,
            encode(CALL_LOG_CONVERSATION)
        )
    }

    pub async fn list_call_logs(&self) -> Result<Vec<CallLogEntry>> {
        let answer = self
            .transport
            .send(Request::get(self.call_logs_url()))
            .await?;
        ensure_success(&answer)?;
        parse_call_logs(&answer.body)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use serde_json::json;
    use session::{ApiResponse, Method};

    use super::*;

    struct Canned {
        body: Value,
        requests: Mutex<Vec<Request>>,
    }

    impl MessageTransport for Canned {
        async fn send(&self, request: Request) -> Result<ApiResponse> {
            self.requests.lock().unwrap().push(request);
            Ok(ApiResponse {
                status: 200,
                body: self.body.clone(),
                retry_after: None,
            })
        }
    }

    fn message(log: Value) -> Value {
        json!({"messagetype": "Text", "properties": {"call-log": log.to_string()}})
    }

    fn log(direction: &str, state: &str, connect: Option<&str>, end: &str) -> Value {
        json!({
            "callId": format!("{direction}-{state}-{end}"),
            "startTime": "2026-10-09T08:00:00.000Z",
            "connectTime": connect,
            "endTime": end,
            "callDirection": direction,
            "callType": "twoParty",
            "callState": state,
            "originator": "8:orgid:caller",
            "target": "8:orgid:callee",
            "originatorParticipant": {"id": "8:orgid:caller", "displayName": "Ada", "type": "user"},
            "targetParticipant": {"id": "8:orgid:callee", "displayName": null, "type": "user"},
        })
    }

    fn parse(logs: Vec<Value>) -> Vec<CallLogEntry> {
        let messages: Vec<Value> = logs.into_iter().map(message).collect();
        parse_call_logs(&json!({"messages": messages})).unwrap()
    }

    #[test]
    fn an_accepted_incoming_call_has_the_caller_and_a_duration() {
        let entries = parse(vec![log(
            "incoming",
            "accepted",
            Some("2026-10-09T08:00:05.000Z"),
            "2026-10-09T08:12:05.000Z",
        )]);
        let entry = &entries[0];
        assert_eq!(entry.direction, CallDirection::Incoming);
        assert_eq!(entry.outcome, CallOutcome::Accepted);
        assert_eq!(entry.duration_seconds, Some(720));
        assert_eq!(entry.peer_id.as_deref(), Some("8:orgid:caller"));
        assert_eq!(entry.peer_name.as_deref(), Some("Ada"));
        assert!(!entry.missed());
        assert!(!entry.is_meeting);
    }

    #[test]
    fn an_outgoing_call_names_the_target_even_without_a_display_name() {
        let entries = parse(vec![log(
            "outgoing",
            "accepted",
            Some("2026-10-09T08:00:05.000Z"),
            "2026-10-09T08:04:17.000Z",
        )]);
        assert_eq!(entries[0].direction, CallDirection::Outgoing);
        assert_eq!(entries[0].duration_seconds, Some(252));
        assert_eq!(entries[0].peer_id.as_deref(), Some("8:orgid:callee"));
        assert_eq!(entries[0].peer_name, None);
    }

    #[test]
    fn a_missed_incoming_call_has_no_connect_time_and_no_duration() {
        let entries = parse(vec![log(
            "incoming",
            "missed",
            None,
            "2026-10-09T08:00:30.000Z",
        )]);
        assert!(entries[0].missed());
        assert_eq!(entries[0].duration_seconds, None);
    }

    #[test]
    fn a_declined_call_and_an_unanswered_outgoing_call_are_not_missed_calls() {
        let entries = parse(vec![
            log("incoming", "declined", None, "2026-10-09T08:00:10.000Z"),
            log("outgoing", "missed", None, "2026-10-09T08:00:40.000Z"),
        ]);
        assert!(entries.iter().all(|entry| !entry.missed()));
        assert_eq!(entries[0].outcome, CallOutcome::Declined);
    }

    #[test]
    fn a_multi_party_call_is_a_meeting_with_its_thread() {
        let mut meeting = log(
            "incoming",
            "accepted",
            Some("2026-10-09T08:00:05.000Z"),
            "2026-10-09T08:38:05.000Z",
        );
        meeting["callType"] = json!("multiParty");
        meeting["threadId"] = json!("19:meeting_x@thread.v2");
        let entries = parse(vec![meeting]);
        assert!(entries[0].is_meeting);
        assert_eq!(
            entries[0].thread_id.as_deref(),
            Some("19:meeting_x@thread.v2")
        );
        assert_eq!(entries[0].duration_seconds, Some(2280));
    }

    #[test]
    fn a_voicemail_target_is_no_peer() {
        let mut forwarded = log("outgoing", "missed", None, "2026-10-09T08:00:40.000Z");
        forwarded["targetParticipant"]["type"] = json!("voicemail");
        let entries = parse(vec![forwarded]);
        assert_eq!(entries[0].peer_id, None);
    }

    #[test]
    fn entries_sort_newest_first_and_skip_other_messages_and_duplicates() {
        let mut older = log("incoming", "missed", None, "2026-10-08T08:00:30.000Z");
        older["callId"] = json!("older");
        older["startTime"] = json!("2026-10-08T08:00:00.000Z");
        let newer = log("incoming", "missed", None, "2026-10-09T08:00:30.000Z");
        let body = json!({"messages": [
            message(older),
            message(newer.clone()),
            message(newer),
            {"messagetype": "RichText/Media_CallLogRecording", "properties": {}},
            {"messagetype": "Text", "properties": {"call-log": "not json"}},
        ]});
        let entries = parse_call_logs(&body).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].call_id, "older");
        assert!(entries[0].started_at > entries[1].started_at);
    }

    #[test]
    fn a_body_without_messages_is_unexpected() {
        assert!(matches!(
            parse_call_logs(&json!({})),
            Err(Error::UnexpectedAnswer(_))
        ));
    }

    #[tokio::test]
    async fn the_list_reads_the_calllogs_conversation() {
        let messages = Messages::with_transport(
            Canned {
                body: json!({"messages": []}),
                requests: Mutex::new(Vec::new()),
            },
            "emea",
        );
        assert!(messages.list_call_logs().await.unwrap().is_empty());
        let requests = messages.transport.requests.lock().unwrap();
        assert_eq!(requests[0].method, Method::Get);
        assert_eq!(
            requests[0].url,
            "https://teams.cloud.microsoft/api/chatsvc/emea/v1/users/ME/conversations/48%3Acalllogs/messages?view=msnp24Equivalent&pageSize=200"
        );
    }
}
