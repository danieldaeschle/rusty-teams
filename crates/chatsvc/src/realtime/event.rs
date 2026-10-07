use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::error::{Error, Result};

const MAX_FIELD_LENGTH: usize = 256;

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
    Presence,
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
pub enum RealtimeEvent {
    Message(MessageEvent),
    Status(StatusEvent),
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
    },
    Status {
        kind: StatusKind,
        #[serde(default)]
        detail: String,
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
        } => {
            let received_at = DateTime::from_timestamp_millis(received_at)
                .ok_or_else(|| Error::Decode("receivedAt out of range".into()))?;
            Ok(RealtimeEvent::Message(MessageEvent {
                resource_type: bounded(resource_type),
                kind: event_kind,
                conversation_id: conversation_id
                    .map(bounded)
                    .filter(|value| !value.is_empty()),
                message_id: message_id.map(bounded).filter(|value| !value.is_empty()),
                received_at,
            }))
        }
        Wire::Status { kind, detail } => Ok(RealtimeEvent::Status(StatusEvent {
            kind,
            detail: bounded(detail),
        })),
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
        let payload = r#"{"channel":"event","resourceType":"presence","eventKind":"presence",
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
    fn rejects_garbage() {
        assert!(decode_payload("not json").is_err());
        assert!(decode_payload(r#"{"channel":"other"}"#).is_err());
        assert!(decode_payload(r#"{"channel":"status","kind":"weird"}"#).is_err());
        assert!(decode_payload(r#"{"channel":"event","resourceType":"x","eventKind":"typing","receivedAt":9223372036854775807}"#).is_err());
    }
}
