use std::collections::BTreeMap;
use std::io::Read;

use base64::Engine;
use flate2::read::{DeflateDecoder, GzDecoder, ZlibDecoder};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::hold::offer_sends_video;

pub const EVT_TEAMS_CALL: i64 = 107;
pub const EVT_GROUP_VIDEO_CALL: i64 = 109;
const MAX_INFLATED_BYTES: u64 = 8 * 1024 * 1024;
const VIDEO_MODALITY: &str = "Video";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    pub mri: String,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CallNotification {
    pub call_id: String,
    pub participant_id: String,
    pub caller: Caller,
    pub links: BTreeMap<String, String>,
    pub offer_sdp: Option<String>,
    pub controller_name: Option<String>,
    pub conversation_controller: Option<String>,
    pub is_multi_party: bool,
    pub subject: Option<String>,
    pub thread_id: Option<String>,
    pub video: bool,
}

impl CallNotification {
    pub fn dedupe_key(&self) -> (String, String) {
        (self.call_id.clone(), self.participant_id.clone())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PushEvent {
    IncomingCall(Box<CallNotification>),
    Other(i64),
}

/// `body` is the raw Trouter request body: `{evt, gp}` or `{evt, cp}`.
pub fn decode_push(body: &str) -> Result<PushEvent> {
    let push: Value = serde_json::from_str(body).map_err(|error| Error::Callback(format!("push is not JSON: {error}")))?;
    let evt = push["evt"].as_i64().ok_or_else(|| Error::Callback("push without evt".into()))?;
    if evt != EVT_TEAMS_CALL && evt != EVT_GROUP_VIDEO_CALL {
        return Ok(PushEvent::Other(evt));
    }
    let payload = payload_of(&push)?;
    Ok(PushEvent::IncomingCall(Box::new(notification_from(&payload, evt)?)))
}

fn payload_of(push: &Value) -> Result<Value> {
    if let Some(plain) = push["gp"].as_str() {
        return serde_json::from_slice(&base64_bytes(plain)?)
            .map_err(|error| Error::Callback(format!("gp is not JSON: {error}")));
    }
    if let Some(compressed) = push["cp"].as_str() {
        let bytes = base64_bytes(compressed)?;
        return inflated_json(&bytes).ok_or_else(|| Error::Callback("cp is not deflated JSON".into()));
    }
    Err(Error::Callback("push has neither gp nor cp".into()))
}

fn base64_bytes(text: &str) -> Result<Vec<u8>> {
    base64::engine::general_purpose::STANDARD
        .decode(text.trim())
        .map_err(|error| Error::Callback(format!("push payload is not base64: {error}")))
}

fn inflated_json(bytes: &[u8]) -> Option<Value> {
    json_from(ZlibDecoder::new(bytes))
        .or_else(|| json_from(DeflateDecoder::new(bytes)))
        .or_else(|| json_from(GzDecoder::new(bytes)))
}

fn json_from(reader: impl Read) -> Option<Value> {
    serde_json::from_str(&read_all(reader)?).ok()
}

fn read_all(reader: impl Read) -> Option<String> {
    let mut text = String::new();
    reader.take(MAX_INFLATED_BYTES).read_to_string(&mut text).ok()?;
    Some(text)
}

fn text_of(value: &Value) -> Option<String> {
    value.as_str().filter(|text| !text.is_empty()).map(str::to_owned)
}

fn notification_from(payload: &Value, evt: i64) -> Result<CallNotification> {
    let notification = &payload["callNotification"];
    let caller_mri = text_of(&notification["from"]["id"])
        .ok_or_else(|| Error::Callback("call notification without caller".into()))?;
    let links: BTreeMap<String, String> = notification["links"]
        .as_object()
        .map(|links| {
            links
                .iter()
                .filter_map(|(name, url)| Some((name.clone(), url.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    let attach = links
        .get("attach")
        .ok_or_else(|| Error::Callback("call notification without attach link".into()))?;
    let participant_id = text_of(&payload["debugContent"]["participantId"])
        .or_else(|| text_of(&notification["to"]["participantId"]))
        .unwrap_or_default();
    let call_id = text_of(&payload["debugContent"]["callId"]).unwrap_or_else(|| attach.clone());
    let modalities_have_video = notification["callModalities"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item.as_str() == Some(VIDEO_MODALITY)));
    let invitation = &payload["conversationInvitation"];
    let offer_sdp = text_of(&notification["mediaContent"]["blob"]);
    let offer_has_video = offer_sdp.as_deref().is_some_and(offer_sends_video);
    Ok(CallNotification {
        call_id,
        participant_id,
        caller: Caller {
            mri: caller_mri,
            display_name: notification["from"]["displayName"].as_str().unwrap_or_default().to_owned(),
        },
        links,
        offer_sdp,
        controller_name: text_of(&notification["controllerName"]),
        conversation_controller: text_of(&invitation["conversationController"]),
        is_multi_party: invitation["isMultiParty"].as_bool().unwrap_or(false),
        subject: text_of(&invitation["subject"]),
        thread_id: text_of(&payload["groupChat"]["threadId"]),
        video: evt == EVT_GROUP_VIDEO_CALL || modalities_have_video || offer_has_video,
    })
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::Compression;
    use flate2::write::{DeflateEncoder, ZlibEncoder};
    use serde_json::json;

    use super::*;

    fn payload() -> Value {
        json!({
            "callNotification": {
                "from": {"id": "8:orgid:caller", "displayName": "Cara"},
                "to": {"participantId": "to-participant"},
                "links": {"attach": "https://cc.skype.com/attach", "redirection": "https://cc.skype.com/redirect"},
                "controllerName": "mixer",
            },
            "conversationInvitation": {"conversationController": "https://conv.skype.com/c", "isMultiParty": false},
            "groupChat": {"threadId": "19:a_b@unq.gbl.spaces"},
            "debugContent": {"callId": "call-1", "participantId": "participant-1"},
        })
    }

    fn base64_of(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    fn gp_body(payload: &Value) -> String {
        json!({"evt": 107, "gp": base64_of(payload.to_string().as_bytes())}).to_string()
    }

    fn incoming(body: &str) -> CallNotification {
        match decode_push(body).unwrap() {
            PushEvent::IncomingCall(notification) => *notification,
            other => panic!("not a call: {other:?}"),
        }
    }

    #[test]
    fn a_plain_push_decodes_to_the_notification() {
        let notification = incoming(&gp_body(&payload()));
        assert_eq!(notification.caller, Caller { mri: "8:orgid:caller".into(), display_name: "Cara".into() });
        assert_eq!(notification.call_id, "call-1");
        assert_eq!(notification.participant_id, "participant-1");
        assert_eq!(notification.links["attach"], "https://cc.skype.com/attach");
        assert_eq!(notification.thread_id.as_deref(), Some("19:a_b@unq.gbl.spaces"));
        assert_eq!(notification.conversation_controller.as_deref(), Some("https://conv.skype.com/c"));
        assert!(!notification.video && !notification.is_multi_party);
        assert!(notification.offer_sdp.is_none());
        assert_eq!(notification.dedupe_key(), ("call-1".to_owned(), "participant-1".to_owned()));
    }

    #[test]
    fn compressed_pushes_decode_with_zlib_and_raw_deflate() {
        let text = payload().to_string();
        let mut zlib = ZlibEncoder::new(Vec::new(), Compression::default());
        zlib.write_all(text.as_bytes()).unwrap();
        let mut raw = DeflateEncoder::new(Vec::new(), Compression::default());
        raw.write_all(text.as_bytes()).unwrap();
        for compressed in [zlib.finish().unwrap(), raw.finish().unwrap()] {
            let body = json!({"evt": 107, "cp": base64_of(&compressed)}).to_string();
            assert_eq!(incoming(&body).call_id, "call-1");
        }
    }

    #[test]
    fn group_video_pushes_and_video_modalities_are_video_calls() {
        let body = json!({"evt": 109, "gp": base64_of(payload().to_string().as_bytes())}).to_string();
        assert!(incoming(&body).video);
        let mut with_video = payload();
        with_video["callNotification"]["callModalities"] = json!(["Audio", "Video"]);
        assert!(incoming(&gp_body(&with_video)).video);
    }

    #[test]
    fn an_offer_that_sends_video_makes_a_video_call_even_without_the_modality() {
        let mut with_video = payload();
        let offer = "v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\nm=audio 1234 RTP/SAVP 111\r\na=mid:0\r\na=sendrecv\r\nm=video 1234 RTP/SAVP 102\r\na=mid:1\r\na=sendrecv\r\na=label:main-video\r\n";
        with_video["callNotification"]["mediaContent"] = json!({"blob": offer});
        assert!(incoming(&gp_body(&with_video)).video);
        with_video["callNotification"]["mediaContent"] = json!({"blob": offer.replace("a=sendrecv\r\na=label:main-video", "a=inactive\r\na=label:main-video")});
        assert!(!incoming(&gp_body(&with_video)).video);
    }

    #[test]
    fn an_offer_in_the_push_is_kept() {
        let mut with_offer = payload();
        with_offer["callNotification"]["mediaContent"] = json!({"blob": "v=0\r\n"});
        assert_eq!(incoming(&gp_body(&with_offer)).offer_sdp.as_deref(), Some("v=0\r\n"));
    }

    #[test]
    fn other_events_are_passed_on_without_decoding() {
        assert_eq!(decode_push(r#"{"evt":115,"cp":"x"}"#).unwrap(), PushEvent::Other(115));
        assert_eq!(decode_push(r#"{"evt":116}"#).unwrap(), PushEvent::Other(116));
    }

    #[test]
    fn broken_pushes_are_errors() {
        assert!(decode_push("garbage").is_err());
        assert!(decode_push(r#"{"gp":"x"}"#).is_err());
        assert!(decode_push(r#"{"evt":107}"#).is_err());
        assert!(decode_push(r#"{"evt":107,"gp":"!!"}"#).is_err());
        let mut without_attach = payload();
        without_attach["callNotification"]["links"] = json!({});
        assert!(decode_push(&gp_body(&without_attach)).is_err());
        let mut without_caller = payload();
        without_caller["callNotification"]["from"] = json!({});
        assert!(decode_push(&gp_body(&without_caller)).is_err());
    }
}
