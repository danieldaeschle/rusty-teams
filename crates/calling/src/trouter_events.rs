use std::collections::BTreeMap;
use std::io::Read;

use base64::Engine;
use chatsvc::TrouterCallback;
use flate2::read::GzDecoder;
use serde_json::{Value, json};

use crate::error::{Error, Result};

const CALLBACK_ROOT: &str = "callAgent/";
const GZIP_ENCODING: &str = "gzip";
const MAX_INFLATED_BYTES: u64 = 8 * 1024 * 1024;

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Acceptance {
    pub sdp: String,
    pub from_mixer: bool,
    pub links: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CallEvent {
    ConversationUpdate(Value),
    RosterUpdate(Value),
    Acceptance(Acceptance),
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
            CallEvent::AddParticipantSuccess(_) => "conversation/addParticipantSuccess".into(),
            CallEvent::AddParticipantFailure(_) => "conversation/addParticipantFailure".into(),
            CallEvent::End(_) => "call/end".into(),
            CallEvent::Other { scope, event } => format!("{scope}/{event}"),
        }
    }
}

/// `path` is `callAgent/<callId>/<tag>/<call|conversation>/<event>/`.
pub fn classify(path: &str, body: Value) -> Result<(String, CallEvent)> {
    let rest = path
        .strip_prefix(CALLBACK_ROOT)
        .ok_or_else(|| Error::Callback(format!("not a call callback: {path}")))?;
    let parts: Vec<&str> = rest.trim_end_matches('/').split('/').collect();
    let [call_id, _tag, scope, event] = parts.as_slice() else {
        return Err(Error::Callback(format!("unexpected callback path shape ({} parts)", parts.len())));
    };
    let event = match (*scope, *event) {
        ("conversation", "conversationUpdate") => CallEvent::ConversationUpdate(body),
        ("conversation", "rosterUpdate") => CallEvent::RosterUpdate(body),
        ("conversation", "addParticipantSuccess") => CallEvent::AddParticipantSuccess(body),
        ("conversation", "addParticipantFailure") => CallEvent::AddParticipantFailure(body),
        ("call", "acceptance") => CallEvent::Acceptance(acceptance(&body)?),
        ("call", "end") => CallEvent::End(call_end(&body)),
        (scope, event) => CallEvent::Other {
            scope: scope.to_owned(),
            event: event.to_owned(),
        },
    };
    Ok((call_id.to_string(), event))
}

fn acceptance(body: &Value) -> Result<Acceptance> {
    let accepted = &body["callAcceptance"];
    let media = &accepted["mediaContent"];
    let sdp = media["blob"]
        .as_str()
        .ok_or_else(|| Error::Callback("acceptance without mediaContent.blob".into()))?
        .to_owned();
    let links = accepted["links"]
        .as_object()
        .map(|links| {
            links
                .iter()
                .filter_map(|(name, url)| Some((name.clone(), url.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    Ok(Acceptance {
        sdp,
        from_mixer: media["fromMixer"].as_bool().unwrap_or(false),
        links,
    })
}

fn call_end(body: &Value) -> CallEnd {
    let end = &body["callEnd"];
    CallEnd {
        code: end["code"].as_i64().unwrap_or_default(),
        sub_code: end["subCode"].as_i64().unwrap_or_default(),
        phrase: end["phrase"].as_str().unwrap_or_default().to_owned(),
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
    let names = [
        "mediaRenegotiation",
        "transfer",
        "replacement",
        "balanceUpdate",
        "retargetCompletion",
        "controlVideoStreaming",
        "updateMediaDescriptions",
    ];
    let links: serde_json::Map<String, Value> = names
        .into_iter()
        .map(|name| (name.to_owned(), Value::String(links.call(name))))
        .collect();
    json!({"callAcceptanceAcknowledgement": {"links": links}})
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
        assert_eq!(end, CallEvent::End(CallEnd { code: 0, sub_code: 5, phrase: "x".into() }));
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
}
