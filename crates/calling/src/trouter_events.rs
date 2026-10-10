use std::collections::BTreeMap;
use std::io::Read;

use base64::Engine;
use chatsvc::TrouterCallback;
use flate2::read::GzDecoder;
use serde_json::{Value, json};

use crate::end::{EndKind, classify_end};
use crate::error::{Error, Result};

const CALLBACK_ROOT: &str = "callAgent/";
const GZIP_ENCODING: &str = "gzip";
const MAX_INFLATED_BYTES: u64 = 8 * 1024 * 1024;
const SPEAKER_KEYS: [&str; 7] = [
    "csrcs",
    "csrc",
    "sources",
    "sourceIds",
    "dominantSpeakers",
    "activeSpeakers",
    "speakers",
];
const MAX_SPEAKER_DEPTH: usize = 4;

pub fn decode_body(callback: &TrouterCallback) -> Result<Value> {
    let text = if callback
        .content_encoding
        .as_deref()
        .is_some_and(|encoding| encoding.eq_ignore_ascii_case(GZIP_ENCODING))
    {
        inflate(&callback.body)?
    } else {
        callback.body.clone()
    };
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&text).map_err(|error| Error::Callback(format!("body is not JSON: {error}")))
}

fn inflate(body: &str) -> Result<String> {
    let compressed = base64::engine::general_purpose::STANDARD
        .decode(body.trim())
        .map_err(|error| Error::Callback(format!("body is not base64: {error}")))?;
    let mut text = String::new();
    GzDecoder::new(compressed.as_slice())
        .take(MAX_INFLATED_BYTES)
        .read_to_string(&mut text)
        .map_err(|error| Error::Callback(format!("body is not gzip: {error}")))?;
    Ok(text)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallEnd {
    pub code: i64,
    pub sub_code: i64,
    pub phrase: String,
    pub accepted_elsewhere_by: Option<String>,
}

impl CallEnd {
    pub fn kind(&self) -> EndKind {
        classify_end(self.code, self.sub_code)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Acceptance {
    pub sdp: String,
    pub from_mixer: bool,
    pub links: BTreeMap<String, String>,
    pub controller_name: Option<String>,
    pub keep_alive_seconds: Option<u64>,
}

impl Acceptance {
    pub fn in_lobby(&self) -> bool {
        self.controller_name.as_deref() == Some(LOBBY_CONTROLLER)
    }
}

pub const LOBBY_CONTROLLER: &str = "lobby";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgressStatus {
    Ringing,
    Forwarded,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaAnswer {
    pub sdp: String,
    pub links: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Renegotiation {
    pub sdp: String,
    pub new_offer: bool,
    pub escalation: bool,
    pub links: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CallEvent {
    ConversationUpdate(Value),
    RosterUpdate(Value),
    Acceptance(Acceptance),
    Progress(ProgressStatus),
    MediaAnswer(MediaAnswer),
    MediaRenegotiation(Renegotiation),
    Speakers(Vec<u32>),
    AddParticipantSuccess(Value),
    AddParticipantFailure(Value),
    End(CallEnd),
    Other { scope: String, event: String },
}

impl CallEvent {
    pub fn name(&self) -> String {
        match self {
            CallEvent::ConversationUpdate(_) => "conversation/conversationUpdate".into(),
            CallEvent::RosterUpdate(_) => "conversation/rosterUpdate".into(),
            CallEvent::Acceptance(_) => "call/acceptance".into(),
            CallEvent::Progress(_) => "call/progress".into(),
            CallEvent::MediaAnswer(_) => "call/mediaAnswer".into(),
            CallEvent::MediaRenegotiation(_) => "call/mediaRenegotiation".into(),
            CallEvent::Speakers(_) => "call/speakers".into(),
            CallEvent::AddParticipantSuccess(_) => "conversation/addParticipantSuccess".into(),
            CallEvent::AddParticipantFailure(_) => "conversation/addParticipantFailure".into(),
            CallEvent::End(_) => "call/end".into(),
            CallEvent::Other { scope, event } => format!("{scope}/{event}"),
        }
    }
}

/// `path` is `callAgent/<callId>/<tag>/<call|conversation>/<event>/`.
pub fn classify(path: &str, body: Value) -> Result<(String, CallEvent)> {
    let (call_id, scope, event) = split_callback_path(path)?;
    let event = match (scope.as_str(), event.as_str()) {
        ("conversation", "conversationUpdate") => CallEvent::ConversationUpdate(body),
        ("conversation", "rosterUpdate") => CallEvent::RosterUpdate(body),
        ("conversation", "addParticipantSuccess") => CallEvent::AddParticipantSuccess(body),
        ("conversation", "addParticipantFailure") => CallEvent::AddParticipantFailure(body),
        ("call", "acceptance") => CallEvent::Acceptance(acceptance(&body)?),
        ("call", "progress") => CallEvent::Progress(progress(&body)),
        ("call", "mediaAnswer") => CallEvent::MediaAnswer(media_answer(&body)?),
        ("call", "mediaRenegotiation") => CallEvent::MediaRenegotiation(renegotiation(&body)?),
        ("call", "csrcInfo" | "dominantSpeakerInfo") => CallEvent::Speakers(speaker_sources(&body)),
        ("call", "end") => CallEvent::End(call_end(&body)),
        (scope, event) => CallEvent::Other {
            scope: scope.to_owned(),
            event: event.to_owned(),
        },
    };
    Ok((call_id, event))
}

pub fn callback_call_id(path: &str) -> Option<String> {
    split_callback_path(path).ok().map(|(call_id, _, _)| call_id)
}

fn split_callback_path(path: &str) -> Result<(String, String, String)> {
    let rest = path
        .strip_prefix(CALLBACK_ROOT)
        .ok_or_else(|| Error::Callback(format!("not a call callback: {path}")))?;
    let parts: Vec<&str> = rest.trim_end_matches('/').split('/').collect();
    let [call_id, _tag, scope, event] = parts.as_slice() else {
        return Err(Error::Callback(format!("unexpected callback path shape ({} parts)", parts.len())));
    };
    Ok(((*call_id).to_owned(), (*scope).to_owned(), (*event).to_owned()))
}

fn string_links(value: &Value) -> BTreeMap<String, String> {
    value
        .as_object()
        .map(|links| {
            links
                .iter()
                .filter_map(|(name, url)| Some((name.clone(), url.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

fn acceptance(body: &Value) -> Result<Acceptance> {
    let accepted = &body["callAcceptance"];
    let media = &accepted["mediaContent"];
    let sdp = media["blob"]
        .as_str()
        .ok_or_else(|| Error::Callback("acceptance without mediaContent.blob".into()))?
        .to_owned();
    Ok(Acceptance {
        sdp,
        from_mixer: media["fromMixer"].as_bool().unwrap_or(false),
        links: string_links(&accepted["links"]),
        controller_name: accepted["controllerName"].as_str().map(str::to_owned),
        keep_alive_seconds: accepted["callKeepAliveInterval"].as_u64(),
    })
}

fn progress(body: &Value) -> ProgressStatus {
    match body["callProgress"]["status"].as_str().map(str::to_ascii_lowercase).as_deref() {
        Some("ringing") => ProgressStatus::Ringing,
        Some("forwarded") => ProgressStatus::Forwarded,
        other => ProgressStatus::Other(other.unwrap_or_default().to_owned()),
    }
}

fn media_answer(body: &Value) -> Result<MediaAnswer> {
    let answer = &body["mediaAnswer"];
    let sdp = answer["mediaContent"]["blob"]
        .as_str()
        .ok_or_else(|| Error::Callback("mediaAnswer without mediaContent.blob".into()))?
        .to_owned();
    Ok(MediaAnswer {
        sdp,
        links: string_links(&answer["links"]),
    })
}

/// The renegotiation message nests its offer under a key the captures do not show, so any object holding `mediaContent` counts.
fn media_holder(body: &Value) -> Option<&Value> {
    if body.get("mediaContent").is_some() {
        return Some(body);
    }
    body.as_object()?.values().find(|value| value.get("mediaContent").is_some())
}

fn renegotiation(body: &Value) -> Result<Renegotiation> {
    let holder = media_holder(body)
        .ok_or_else(|| Error::Callback("mediaRenegotiation without mediaContent".into()))?;
    let media = &holder["mediaContent"];
    let sdp = media["blob"]
        .as_str()
        .ok_or_else(|| Error::Callback("mediaRenegotiation without mediaContent.blob".into()))?
        .to_owned();
    Ok(Renegotiation {
        sdp,
        new_offer: media["newOffer"].as_bool().unwrap_or(false),
        escalation: media["escalationOccurring"].as_bool().unwrap_or(false)
            || media["isTwoPartyToMultiPartyEscalation"].as_bool().unwrap_or(false),
        links: string_links(&holder["links"]),
    })
}

/// Speaker lists come as arrays of source ids under keys the captures do not show.
pub fn speaker_sources(body: &Value) -> Vec<u32> {
    let mut sources = Vec::new();
    collect_sources(body, 0, &mut sources);
    sources.sort_unstable();
    sources.dedup();
    sources
}

fn collect_sources(value: &Value, depth: usize, sources: &mut Vec<u32>) {
    if depth > MAX_SPEAKER_DEPTH {
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if SPEAKER_KEYS.contains(&key.as_str()) {
                    push_ids(child, sources);
                } else {
                    collect_sources(child, depth + 1, sources);
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|item| collect_sources(item, depth + 1, sources)),
        _ => {}
    }
}

fn push_ids(value: &Value, sources: &mut Vec<u32>) {
    match value {
        Value::Number(number) => sources.extend(number.as_u64().and_then(|id| u32::try_from(id).ok())),
        Value::Array(items) => items.iter().for_each(|item| push_ids(item, sources)),
        Value::Object(map) => {
            if let Some(id) = map.get("sourceId") {
                push_ids(id, sources);
            }
        }
        _ => {}
    }
}

fn call_end(body: &Value) -> CallEnd {
    let end = &body["callEnd"];
    CallEnd {
        code: end["code"].as_i64().unwrap_or_default(),
        sub_code: end["subCode"].as_i64().unwrap_or_default(),
        phrase: end["phrase"].as_str().unwrap_or_default().to_owned(),
        accepted_elsewhere_by: end["acceptedElsewhereBy"]["displayName"]
            .as_str()
            .or_else(|| end["acceptedElsewhereBy"]["id"].as_str())
            .map(str::to_owned),
    }
}

/// Callback URLs the client invents under its Trouter `surl`; Trouter routes them to the socket without registration.
#[derive(Debug, Clone)]
pub struct CallbackLinks {
    base: String,
    call_id: String,
}

impl CallbackLinks {
    pub fn new(surl: &str, call_id: &str) -> Self {
        let mut base = surl.to_owned();
        if !base.ends_with('/') {
            base.push('/');
        }
        CallbackLinks {
            base,
            call_id: call_id.to_owned(),
        }
    }

    pub fn call_id(&self) -> &str {
        &self.call_id
    }

    pub fn link(&self, scope: &str, event: &str) -> String {
        let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];
        format!("{}{CALLBACK_ROOT}{}/{tag}/{scope}/{event}/", self.base, self.call_id)
    }

    pub fn conversation(&self, event: &str) -> String {
        self.link("conversation", event)
    }

    pub fn call(&self, event: &str) -> String {
        self.link("call", event)
    }
}

pub fn acceptance_acknowledgement(links: &CallbackLinks) -> Value {
    json!({"callAcceptanceAcknowledgement": {"links": acceptance_links(links)}})
}

pub fn acceptance_links(links: &CallbackLinks) -> serde_json::Map<String, Value> {
    let names = [
        "mediaRenegotiation",
        "transfer",
        "replacement",
        "balanceUpdate",
        "retargetCompletion",
        "controlVideoStreaming",
        "updateMediaDescriptions",
    ];
    names
        .into_iter()
        .map(|name| (name.to_owned(), Value::String(links.call(name))))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::Compression;
    use flate2::write::GzEncoder;

    use super::*;

    fn gzip_base64(text: &str) -> String {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(text.as_bytes()).unwrap();
        base64::engine::general_purpose::STANDARD.encode(encoder.finish().unwrap())
    }

    fn callback(path: &str, body: &str, gzip: bool) -> TrouterCallback {
        TrouterCallback {
            request_id: 1,
            path: path.into(),
            content_encoding: gzip.then(|| "gzip".into()),
            body: if gzip { gzip_base64(body) } else { body.into() },
        }
    }

    #[test]
    fn gzip_base64_bodies_decode_to_json() {
        let body = r#"{"callEnd":{"code":0,"subCode":0,"phrase":"CallEndReasonLocalUserInitiated"}}"#;
        let decoded = decode_body(&callback("callAgent/c/1/call/end/", body, true)).unwrap();
        assert_eq!(decoded["callEnd"]["phrase"], "CallEndReasonLocalUserInitiated");
        let plain = decode_body(&callback("callAgent/c/1/call/end/", body, false)).unwrap();
        assert_eq!(plain, decoded);
    }

    #[test]
    fn classifies_acceptance_with_sdp_and_links() {
        let body = json!({"callAcceptance": {"mediaContent": {"blob": "v=0\r\n", "fromMixer": true},
            "links": {"acknowledgement": "https://cc/ack", "callLeg": "https://cc/leg"}}});
        let (call_id, event) = classify("callAgent/abc/dbea1786/call/acceptance/", body).unwrap();
        assert_eq!(call_id, "abc");
        let CallEvent::Acceptance(acceptance) = event else { panic!("not acceptance") };
        assert_eq!(acceptance.sdp, "v=0\r\n");
        assert!(acceptance.from_mixer);
        assert_eq!(acceptance.links["callLeg"], "https://cc/leg");
    }

    #[test]
    fn classifies_end_and_unknown_events() {
        let (_, end) = classify("callAgent/abc/1/call/end/", json!({"callEnd": {"code": 0, "subCode": 5, "phrase": "x"}})).unwrap();
        assert_eq!(end, CallEvent::End(CallEnd { code: 0, sub_code: 5, phrase: "x".into(), accepted_elsewhere_by: None }));
        let (_, other) = classify("callAgent/abc/1/call/updateMediaDescriptions", Value::Null).unwrap();
        assert_eq!(other.name(), "call/updateMediaDescriptions");
        assert!(classify("unifiedPresenceService", Value::Null).is_err());
    }

    #[test]
    fn links_live_under_the_socket_url() {
        let links = CallbackLinks::new("https://pub-ent-euwe-02-f.trouter.teams.microsoft.com:3443/v4/f/abc", "call-1");
        let link = links.call("acceptance");
        assert!(link.starts_with("https://pub-ent-euwe-02-f.trouter.teams.microsoft.com:3443/v4/f/abc/callAgent/call-1/"));
        assert!(link.ends_with("/call/acceptance/"));
        let acknowledgement = acceptance_acknowledgement(&links);
        assert!(acknowledgement["callAcceptanceAcknowledgement"]["links"]["mediaRenegotiation"]
            .as_str()
            .unwrap()
            .ends_with("/call/mediaRenegotiation/"));
    }
    #[test]
    fn progress_reports_ringing_and_forwarding() {
        let ringing = json!({"callProgress": {"sender": "x", "status": "ringing", "phrase": "ringing"}});
        let (_, event) = classify("callAgent/abc/1/call/progress/", ringing).unwrap();
        assert_eq!(event, CallEvent::Progress(ProgressStatus::Ringing));
        let forwarded = json!({"callProgress": {"status": "forwarded"}});
        let (_, event) = classify("callAgent/abc/1/call/progress/", forwarded).unwrap();
        assert_eq!(event, CallEvent::Progress(ProgressStatus::Forwarded));
        let (_, other) = classify("callAgent/abc/1/call/progress/", json!({})).unwrap();
        assert_eq!(other, CallEvent::Progress(ProgressStatus::Other(String::new())));
    }

    #[test]
    fn provisional_answers_carry_sdp_and_links() {
        let body = json!({"mediaAnswer": {"sender": "x", "mediaContent": {"blob": "v=0\r\n"},
            "links": {"mediaAcknowledgement": "https://cc/ack"}}});
        let (_, event) = classify("callAgent/abc/1/call/mediaAnswer/", body).unwrap();
        let CallEvent::MediaAnswer(answer) = event else { panic!("not a media answer") };
        assert_eq!(answer.sdp, "v=0\r\n");
        assert_eq!(answer.links["mediaAcknowledgement"], "https://cc/ack");
        assert!(classify("callAgent/abc/1/call/mediaAnswer/", json!({})).is_err());
    }

    #[test]
    fn renegotiation_finds_its_offer_under_any_key() {
        let nested = json!({"mediaNegotiation": {"mediaContent": {"blob": "v=0\r\n", "newOffer": true,
            "escalationOccurring": true}, "links": {"mediaAnswer": "https://cc/answer"}}});
        let (_, event) = classify("callAgent/abc/1/call/mediaRenegotiation/", nested).unwrap();
        let CallEvent::MediaRenegotiation(renegotiation) = event else { panic!("not a renegotiation") };
        assert!(renegotiation.new_offer && renegotiation.escalation);
        assert_eq!(renegotiation.links["mediaAnswer"], "https://cc/answer");
        let flat = json!({"mediaContent": {"blob": "v=0\r\n", "isTwoPartyToMultiPartyEscalation": true}});
        let (_, event) = classify("callAgent/abc/1/call/mediaRenegotiation/", flat).unwrap();
        let CallEvent::MediaRenegotiation(renegotiation) = event else { panic!("not a renegotiation") };
        assert!(!renegotiation.new_offer && renegotiation.escalation);
        assert!(classify("callAgent/abc/1/call/mediaRenegotiation/", json!({"x": 1})).is_err());
    }

    #[test]
    fn speaker_sources_come_from_known_list_keys() {
        assert_eq!(speaker_sources(&json!({"csrcInfo": {"csrcs": [201, 205, 201]}})), vec![201, 205]);
        assert_eq!(speaker_sources(&json!({"dominantSpeakers": [{"sourceId": 7}]})), vec![7]);
        assert!(speaker_sources(&json!({"other": [1, 2]})).is_empty());
        let (_, event) = classify("callAgent/abc/1/call/csrcInfo/", json!({"sources": [3]})).unwrap();
        assert_eq!(event, CallEvent::Speakers(vec![3]));
    }

    #[test]
    fn end_names_who_answered_elsewhere() {
        let body = json!({"callEnd": {"code": 450, "subCode": 0, "phrase": "x",
            "acceptedElsewhereBy": {"id": "8:orgid:1", "displayName": "Daniel"}}});
        let (_, event) = classify("callAgent/abc/1/call/end/", body).unwrap();
        let CallEvent::End(end) = event else { panic!("not an end") };
        assert_eq!(end.kind(), EndKind::AnsweredElsewhere);
        assert_eq!(end.accepted_elsewhere_by.as_deref(), Some("Daniel"));
    }

    #[test]
    fn acceptance_in_the_lobby_is_flagged() {
        let body = json!({"callAcceptance": {"controllerName": "lobby", "callKeepAliveInterval": 2700,
            "mediaContent": {"blob": "v=0\r\n"}}});
        let (_, event) = classify("callAgent/abc/1/call/acceptance/", body).unwrap();
        let CallEvent::Acceptance(acceptance) = event else { panic!("not acceptance") };
        assert!(acceptance.in_lobby());
        assert_eq!(acceptance.keep_alive_seconds, Some(2700));
    }

    #[test]
    fn call_ids_come_from_the_path_alone() {
        assert_eq!(callback_call_id("callAgent/abc/1/call/end/").as_deref(), Some("abc"));
        assert_eq!(callback_call_id("unifiedPresenceService"), None);
        assert_eq!(callback_call_id(""), None);
    }
}
