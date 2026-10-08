use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::host::is_trouter_host;
use crate::error::{Error, Result};

const MAX_FIELD_LENGTH: usize = 256;
const ORGID_PREFIX: &str = "8:orgid:";
const HTTPS_PREFIX: &str = "https://";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    NewMessage,
    MessageUpdate,
    ThreadUpdate,
    Typing,
    ReadReceipt,
    ThreadActivity,
    Control,
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageEvent {
    pub resource_type: String,
    pub kind: EventKind,
    pub conversation_id: Option<String>,
    pub message_id: Option<String>,
    pub received_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypingEvent {
    pub conversation_id: String,
    pub user_id: String,
    pub display_name: String,
    pub active: bool,
    pub received_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusKind {
    Connected,
    Disconnected,
    MessageLoss,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEvent {
    pub kind: StatusKind,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresenceUpdate {
    pub user_id: String,
    pub availability: String,
    pub activity: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrouterEndpoint {
    pub endpoint_id: String,
    pub trouter_uri: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RealtimeEvent {
    Message(MessageEvent),
    Typing(TypingEvent),
    Status(StatusEvent),
    Presence(Vec<PresenceUpdate>),
    Endpoint(TrouterEndpoint),
}

#[derive(Deserialize)]
struct WirePresence {
    mri: String,
    availability: String,
    #[serde(default)]
    activity: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "channel", rename_all = "snake_case")]
enum Wire {
    Event {
        #[serde(rename = "resourceType")]
        resource_type: String,
        #[serde(rename = "eventKind")]
        event_kind: EventKind,
        #[serde(rename = "conversationId", default)]
        conversation_id: Option<String>,
        #[serde(rename = "messageId", default)]
        message_id: Option<String>,
        #[serde(rename = "receivedAt")]
        received_at: i64,
        #[serde(default)]
        typing: Option<String>,
        #[serde(rename = "senderId", default)]
        sender_id: Option<String>,
        #[serde(rename = "senderName", default)]
        sender_name: Option<String>,
    },
    Status {
        kind: StatusKind,
        #[serde(default)]
        detail: String,
    },
    Presence {
        entries: Vec<WirePresence>,
    },
    Endpoint {
        #[serde(rename = "endpointId")]
        endpoint_id: String,
        #[serde(rename = "trouterUri")]
        trouter_uri: String,
    },
}

pub fn decode_payload(payload: &str) -> Result<RealtimeEvent> {
    let wire: Wire =
        serde_json::from_str(payload).map_err(|error| Error::Decode(error.to_string()))?;
    match wire {
        Wire::Event {
            resource_type,
            event_kind,
            conversation_id,
            message_id,
            received_at,
            typing,
            sender_id,
            sender_name,
        } => {
            let received_at = DateTime::from_timestamp_millis(received_at)
                .ok_or_else(|| Error::Decode("receivedAt out of range".into()))?;
            let conversation_id = conversation_id
                .map(bounded)
                .filter(|value| !value.is_empty());
            if event_kind == EventKind::Typing
                && let Some(event) = typing_event(
                    conversation_id.clone(),
                    typing.as_deref(),
                    sender_id,
                    sender_name,
                    received_at,
                )
            {
                return Ok(RealtimeEvent::Typing(event));
            }
            Ok(RealtimeEvent::Message(MessageEvent {
                resource_type: bounded(resource_type),
                kind: event_kind,
                conversation_id,
                message_id: message_id.map(bounded).filter(|value| !value.is_empty()),
                received_at,
            }))
        }
        Wire::Status { kind, detail } => Ok(RealtimeEvent::Status(StatusEvent {
            kind,
            detail: bounded(detail),
        })),
        Wire::Presence { entries } => Ok(RealtimeEvent::Presence(
            entries
                .into_iter()
                .filter_map(|entry| {
                    let user_id = entry.mri.strip_prefix(ORGID_PREFIX)?;
                    Some(PresenceUpdate {
                        user_id: bounded(user_id.to_owned()),
                        availability: bounded(entry.availability),
                        activity: entry
                            .activity
                            .map(bounded)
                            .filter(|value| !value.is_empty()),
                    })
                })
                .collect(),
        )),
        Wire::Endpoint {
            endpoint_id,
            trouter_uri,
        } => {
            let host = trouter_uri
                .strip_prefix(HTTPS_PREFIX)
                .and_then(|rest| rest.split('/').next())
                .map(without_port)
                .filter(|host| is_trouter_host(host));
            if host.is_none() || endpoint_id.is_empty() {
                return Err(Error::Decode("endpoint rejected".into()));
            }
            Ok(RealtimeEvent::Endpoint(TrouterEndpoint {
                endpoint_id: bounded(endpoint_id),
                trouter_uri,
            }))
        }
    }
}

fn typing_event(
    conversation_id: Option<String>,
    signal: Option<&str>,
    sender_id: Option<String>,
    sender_name: Option<String>,
    received_at: DateTime<Utc>,
) -> Option<TypingEvent> {
    let active = match signal? {
        "start" => true,
        "clear" => false,
        _ => return None,
    };
    let user_id = bounded(sender_id?);
    if user_id.is_empty() {
        return None;
    }
    Some(TypingEvent {
        conversation_id: conversation_id?,
        user_id,
        display_name: bounded(sender_name.unwrap_or_default()),
        active,
        received_at,
    })
}

fn without_port(authority: &str) -> &str {
    match authority.rsplit_once(':') {
        Some((host, port))
            if !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            host
        }
        _ => authority,
    }
}

fn bounded(value: String) -> String {
    value.chars().take(MAX_FIELD_LENGTH).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_message_event() {
        let payload = r#"{"channel":"event","resourceType":"NewMessage","eventKind":"new_message",
            "conversationId":"19:abc@thread.v2","messageId":"1791368146075","receivedAt":1791368146999}"#;
        let RealtimeEvent::Message(event) = decode_payload(payload).unwrap() else {
            panic!("expected message")
        };
        assert_eq!(event.kind, EventKind::NewMessage);
        assert_eq!(event.resource_type, "NewMessage");
        assert_eq!(event.conversation_id.as_deref(), Some("19:abc@thread.v2"));
        assert_eq!(event.message_id.as_deref(), Some("1791368146075"));
        assert_eq!(event.received_at.timestamp_millis(), 1791368146999);
    }

    #[test]
    fn null_and_empty_ids_become_none() {
        let payload = r#"{"channel":"event","resourceType":"typing","eventKind":"typing",
            "conversationId":null,"messageId":"","receivedAt":1}"#;
        let RealtimeEvent::Message(event) = decode_payload(payload).unwrap() else {
            panic!("expected message")
        };
        assert_eq!(event.conversation_id, None);
        assert_eq!(event.message_id, None);
    }

    #[test]
    fn unknown_event_kind_is_other() {
        let payload =
            r#"{"channel":"event","resourceType":"X","eventKind":"brand_new","receivedAt":1}"#;
        let RealtimeEvent::Message(event) = decode_payload(payload).unwrap() else {
            panic!("expected message")
        };
        assert_eq!(event.kind, EventKind::Other);
    }

    fn typing_payload(extra: &str) -> String {
        format!(
            r#"{{"channel":"event","resourceType":"NewMessage","eventKind":"typing",
            "conversationId":"19:abc@thread.v2","messageId":null,"receivedAt":5{extra}}}"#
        )
    }

    #[test]
    fn decodes_typing_start_and_clear() {
        for (signal, active) in [("start", true), ("clear", false)] {
            let payload = typing_payload(&format!(
                r#","typing":"{signal}","senderId":"abc-1","senderName":"Ada Lovelace""#
            ));
            assert_eq!(
                decode_payload(&payload).unwrap(),
                RealtimeEvent::Typing(TypingEvent {
                    conversation_id: "19:abc@thread.v2".into(),
                    user_id: "abc-1".into(),
                    display_name: "Ada Lovelace".into(),
                    active,
                    received_at: DateTime::from_timestamp_millis(5).unwrap(),
                })
            );
        }
    }

    #[test]
    fn typing_without_a_name_keeps_an_empty_name_and_cuts_long_ones() {
        let RealtimeEvent::Typing(event) =
            decode_payload(&typing_payload(r#","typing":"start","senderId":"a""#)).unwrap()
        else {
            panic!("expected typing")
        };
        assert_eq!(event.display_name, "");
        let long = typing_payload(&format!(
            r#","typing":"start","senderId":"a","senderName":"{}""#,
            "n".repeat(1000)
        ));
        let RealtimeEvent::Typing(event) = decode_payload(&long).unwrap() else {
            panic!("expected typing")
        };
        assert_eq!(event.display_name.chars().count(), MAX_FIELD_LENGTH);
    }

    #[test]
    fn typing_without_a_valid_sender_or_signal_stays_a_plain_message() {
        for extra in [
            "",
            r#","typing":"start""#,
            r#","typing":"start","senderId":null"#,
            r##","typing":"start","senderId":"""##,
            r#","typing":"bogus","senderId":"a""#,
            r#","senderId":"a""#,
        ] {
            let RealtimeEvent::Message(event) = decode_payload(&typing_payload(extra)).unwrap()
            else {
                panic!("expected message for {extra}")
            };
            assert_eq!(event.kind, EventKind::Typing);
        }
    }

    #[test]
    fn typing_without_a_conversation_stays_a_plain_message() {
        let payload = r#"{"channel":"event","resourceType":"NewMessage","eventKind":"typing",
            "conversationId":null,"receivedAt":5,"typing":"start","senderId":"a"}"#;
        assert!(matches!(
            decode_payload(payload).unwrap(),
            RealtimeEvent::Message(_)
        ));
    }

    #[test]
    fn decodes_every_status_kind() {
        for (name, kind) in [
            ("connected", StatusKind::Connected),
            ("disconnected", StatusKind::Disconnected),
            ("message_loss", StatusKind::MessageLoss),
            ("error", StatusKind::Error),
        ] {
            let payload = format!(r#"{{"channel":"status","kind":"{name}","detail":"d"}}"#);
            assert_eq!(
                decode_payload(&payload).unwrap(),
                RealtimeEvent::Status(StatusEvent {
                    kind,
                    detail: "d".into()
                })
            );
        }
    }

    #[test]
    fn status_detail_is_optional() {
        let RealtimeEvent::Status(status) =
            decode_payload(r#"{"channel":"status","kind":"connected"}"#).unwrap()
        else {
            panic!("expected status")
        };
        assert_eq!(status.detail, "");
    }

    #[test]
    fn long_fields_are_cut() {
        let payload = format!(
            r#"{{"channel":"status","kind":"error","detail":"{}"}}"#,
            "x".repeat(1000)
        );
        let RealtimeEvent::Status(status) = decode_payload(&payload).unwrap() else {
            panic!("expected status")
        };
        assert_eq!(status.detail.chars().count(), MAX_FIELD_LENGTH);
    }

    #[test]
    fn decodes_presence_and_skips_foreign_mris() {
        let payload = r#"{"channel":"presence","entries":[
            {"mri":"8:orgid:abc","availability":"Busy","activity":"InACall"},
            {"mri":"8:orgid:def","availability":"Away","activity":""},
            {"mri":"4:+4912345","availability":"Offline"}]}"#;
        let RealtimeEvent::Presence(updates) = decode_payload(payload).unwrap() else {
            panic!("expected presence")
        };
        assert_eq!(
            updates,
            vec![
                PresenceUpdate {
                    user_id: "abc".into(),
                    availability: "Busy".into(),
                    activity: Some("InACall".into()),
                },
                PresenceUpdate {
                    user_id: "def".into(),
                    availability: "Away".into(),
                    activity: None,
                },
            ]
        );
    }

    #[test]
    fn decodes_endpoint_on_a_trouter_host() {
        let payload = r#"{"channel":"endpoint","endpointId":"e1",
            "trouterUri":"https://pub-ent-sece-04-f.trouter.teams.microsoft.com:3443/v4/f/x/unifiedPresenceService"}"#;
        let RealtimeEvent::Endpoint(endpoint) = decode_payload(payload).unwrap() else {
            panic!("expected endpoint")
        };
        assert_eq!(endpoint.endpoint_id, "e1");
        assert!(endpoint.trouter_uri.ends_with("/unifiedPresenceService"));
    }

    #[test]
    fn rejects_endpoints_off_trouter_or_without_https() {
        for uri in [
            "https://evil.example/unifiedPresenceService",
            "http://go-eu.trouter.teams.microsoft.com/x",
            "https://go-eu.trouter.teams.microsoft.com.evil.com/x",
            "https://a@go-eu.trouter.teams.microsoft.com/x",
            "https://go-eu.trouter.teams.microsoft.com:x/x",
            "go-eu.trouter.teams.microsoft.com/x",
        ] {
            let payload =
                format!(r#"{{"channel":"endpoint","endpointId":"e1","trouterUri":"{uri}"}}"#);
            assert!(decode_payload(&payload).is_err(), "{uri}");
        }
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode_payload("not json").is_err());
        assert!(decode_payload(r#"{"channel":"other"}"#).is_err());
        assert!(decode_payload(r#"{"channel":"status","kind":"weird"}"#).is_err());
        assert!(decode_payload(r#"{"channel":"event","resourceType":"x","eventKind":"typing","receivedAt":9223372036854775807}"#).is_err());
    }
}
