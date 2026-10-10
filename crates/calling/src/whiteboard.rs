use serde_json::Value;
use session::{Method, Request, SPACES, Scope, Session};

use crate::error::{Error, Result};
use crate::signaling::{CHATSVC_REGION, MeetingTarget};
use crate::trouter_events::find_key;

const WHITEBOARD_HOST: &str = "whiteboard.microsoft.com";
const BOARD_SCOPE: &str = "user_impersonation";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentShare {
    pub session_id: String,
    pub presenter: Option<String>,
    pub subject: String,
    pub url: Option<String>,
    pub whiteboard: bool,
}

/// A whiteboard share is told apart by the board host in the share URL or by its name.
fn is_whiteboard(identifier: &str, subject: &str, url: Option<&str>) -> bool {
    let named = |text: &str| text.to_ascii_lowercase().contains("whiteboard");
    url.is_some_and(|url| url.contains(WHITEBOARD_HOST)) || named(identifier) || named(subject)
}

pub fn content_share(body: &Value) -> Option<ContentShare> {
    let state = find_key(body, "sessionState")?;
    let text = |key: &str| find_key(body, key).and_then(Value::as_str).unwrap_or_default().to_owned();
    let url = state["url"].as_str().filter(|url| !url.is_empty()).map(str::to_owned);
    let presenter = find_key(body, "presenter").and_then(|presenter| match presenter {
        Value::String(mri) => Some(mri.clone()),
        other => other["id"].as_str().or_else(|| other["mri"].as_str()).map(str::to_owned),
    });
    let (identifier, subject) = (text("identifier"), text("subject"));
    Some(ContentShare {
        session_id: text("sessionId"),
        presenter,
        whiteboard: is_whiteboard(&identifier, &subject, url.as_deref()),
        subject,
        url,
    })
}

fn encoded(component: &str) -> String {
    component
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (byte as char).to_string(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

pub fn board_url(target: &MeetingTarget, meeting_title: &str) -> String {
    format!(
        "https://teams.cloud.microsoft/api/mt/{CHATSVC_REGION}/beta/meetings/fluidboard?threadId={}&messageId=0&organizerId={}&tenantId={}&meetingTitle={}",
        encoded(&target.thread_id),
        encoded(&target.organizer_id),
        encoded(&target.tenant_id),
        encoded(meeting_title),
    )
}

pub async fn fetch_board(session: &Session, target: &MeetingTarget, meeting_title: &str) -> Result<String> {
    let request = Request { method: Method::Get, ..Request::get(board_url(target, meeting_title)) };
    let response = session.send(request, &Scope::new(SPACES, BOARD_SCOPE)).await?;
    if !response.is_success() {
        return Err(Error::Signaling(format!("whiteboard answered HTTP {}", response.status)));
    }
    response.body["url"]
        .as_str()
        .or_else(|| response.body["shareUrl"].as_str())
        .map(str::to_owned)
        .ok_or_else(|| Error::Signaling("whiteboard answer without a url".into()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn share(url: &str, subject: &str) -> Value {
        json!({"contentSharingUpdate": {
            "sessionId": "s-1",
            "identifier": "app-id",
            "presenter": {"id": "8:orgid:ana", "displayName": "Ana"},
            "subject": subject,
            "sequenceNumber": 2,
            "sessionState": {"url": url, "passThrough": "x"},
            "links": {"contentSharingController": "https://css.skype.com/contentshare/1"},
        }})
    }

    #[test]
    fn a_board_url_marks_the_share_as_a_whiteboard() {
        let parsed = content_share(&share("https://app.whiteboard.microsoft.com/me/whiteboards/x?encodedShareLink=1", "Meeting")).unwrap();
        assert!(parsed.whiteboard);
        assert_eq!(parsed.presenter.as_deref(), Some("8:orgid:ana"));
        assert_eq!(parsed.session_id, "s-1");
        assert!(parsed.url.as_deref().unwrap().contains("encodedShareLink"));
    }

    #[test]
    fn the_name_marks_a_whiteboard_when_the_url_does_not() {
        assert!(content_share(&share("https://example.com/board", "Microsoft Whiteboard")).unwrap().whiteboard);
        assert!(!content_share(&share("https://example.com/deck", "Quarterly slides")).unwrap().whiteboard);
    }

    #[test]
    fn a_string_presenter_and_a_missing_url_still_parse() {
        let body = json!({"sessionId": "s", "presenter": "8:orgid:bo", "identifier": "whiteboard", "sessionState": {}});
        let parsed = content_share(&body).unwrap();
        assert_eq!(parsed.presenter.as_deref(), Some("8:orgid:bo"));
        assert!(parsed.url.is_none());
        assert!(parsed.whiteboard);
        assert!(content_share(&json!({"sessionId": "s"})).is_none());
    }

    #[test]
    fn the_board_request_names_the_meeting() {
        let target = MeetingTarget { thread_id: "19:meeting_x@thread.v2".into(), tenant_id: "t-1".into(), organizer_id: "o-1".into(), meeting_data: None };
        let url = board_url(&target, "Q3 plan");
        assert!(url.starts_with("https://teams.cloud.microsoft/api/mt/emea/beta/meetings/fluidboard?"));
        assert!(url.contains("threadId=19%3Ameeting_x%40thread.v2"));
        assert!(url.contains("messageId=0&organizerId=o-1&tenantId=t-1&meetingTitle=Q3%20plan"));
    }
}
