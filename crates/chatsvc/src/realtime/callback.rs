use serde::Deserialize;
use tokio::sync::mpsc;

const CALLBACK_CHANNEL: &str = "callback";
const MAX_CALLBACK_BODY: usize = 4 * 1024 * 1024;

/// A Trouter request for a callback URL the client handed out (`<surl>callAgent/...`), held open until replied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrouterCallback {
    pub request_id: u64,
    pub path: String,
    pub content_encoding: Option<String>,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CallbackReply {
    pub request_id: u64,
    pub status: u16,
    pub body: String,
}

/// Answers held Trouter callbacks; unanswered ones get an empty 200 from the page after a few seconds.
#[derive(Debug, Clone)]
pub struct CallbackReplier {
    pub(super) sender: mpsc::UnboundedSender<CallbackReply>,
}

impl CallbackReplier {
    pub fn reply(&self, request_id: u64, status: u16, body: impl Into<String>) {
        let _ = self.sender.send(CallbackReply {
            request_id,
            status,
            body: body.into(),
        });
    }
}

#[derive(Deserialize)]
struct WireCallback {
    channel: String,
    #[serde(rename = "requestId")]
    request_id: u64,
    path: String,
    #[serde(rename = "contentEncoding", default)]
    content_encoding: Option<String>,
    #[serde(default)]
    body: String,
}

pub(super) fn decode_callback(payload: &str) -> Option<TrouterCallback> {
    if !payload.contains("\"channel\":\"callback\"") {
        return None;
    }
    let wire: WireCallback = serde_json::from_str(payload).ok()?;
    if wire.channel != CALLBACK_CHANNEL || wire.body.len() > MAX_CALLBACK_BODY {
        return None;
    }
    Some(TrouterCallback {
        request_id: wire.request_id,
        path: wire.path,
        content_encoding: wire.content_encoding,
        body: wire.body,
    })
}

pub(super) fn reply_expression(global_name: &str, reply: &CallbackReply) -> String {
    format!(
        "window.{global_name} ? window.{global_name}.reply({}, {}, {}) : false",
        reply.request_id,
        reply.status,
        serde_json::Value::String(reply.body.clone())
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_callback_payload() {
        let payload = r#"{"channel":"callback","requestId":1941608514,"path":"callAgent/a/b/call/acceptance/","contentEncoding":"gzip","body":"H4sI"}"#;
        let callback = decode_callback(payload).unwrap();
        assert_eq!(callback.request_id, 1941608514);
        assert_eq!(callback.path, "callAgent/a/b/call/acceptance/");
        assert_eq!(callback.content_encoding.as_deref(), Some("gzip"));
        assert_eq!(callback.body, "H4sI");
    }

    #[test]
    fn other_channels_are_not_callbacks() {
        assert!(decode_callback(r#"{"channel":"status","kind":"connected"}"#).is_none());
        assert!(decode_callback("garbage").is_none());
    }

    #[test]
    fn reply_body_is_a_js_string_literal() {
        let reply = CallbackReply {
            request_id: 7,
            status: 200,
            body: "{\"a\":\"</script>'\"}".into(),
        };
        assert_eq!(
            reply_expression("__x", &reply),
            r#"window.__x ? window.__x.reply(7, 200, "{\"a\":\"</script>'\"}") : false"#
        );
    }
}
