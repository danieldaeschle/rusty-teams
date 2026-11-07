use calling::MeetingTarget;
use gpui_kit::*;
use serde_json::json;

use crate::app_state::AppState;

pub const META_WEB_CALLS: &str = "calls.teams_web";
const TEAMS_WEB: &str = "https://teams.microsoft.com/l";
const WEB_CALL_TITLE: &str = "Teams call";

pub fn stored_web_calls(store: &store::Store) -> bool {
    store.meta(META_WEB_CALLS).ok().flatten().as_deref() == Some("true")
}

pub fn meeting_web_url(target: &MeetingTarget) -> String {
    let context = json!({"Tid": target.tenant_id, "Oid": target.organizer_id}).to_string();
    format!(
        "{TEAMS_WEB}/meetup-join/{}/0?context={}",
        encode(&target.thread_id),
        encode(&context)
    )
}

/// Teams on the web has no call link for a chat id, so the chat opens and the call starts there.
pub fn chat_web_url(thread_id: &str) -> String {
    format!("{TEAMS_WEB}/chat/{}/0", encode(thread_id))
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            byte => format!("%{byte:02X}"),
        })
        .collect()
}

impl AppState {
    pub fn set_web_calls(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.web_calls = enabled;
        let _ = self
            .store
            .set_meta(META_WEB_CALLS, if enabled { "true" } else { "false" });
        cx.notify();
    }

    /// Opens the call in Teams on the web when that setting is on; false leaves it to the own engine.
    pub(super) fn try_web_call(&mut self, url: String, cx: &mut Context<Self>) -> bool {
        if !self.web_calls || self.mode.demo {
            return false;
        }
        open_web_call(url, cx)
    }
}

#[cfg(windows)]
fn open_web_call(url: String, cx: &mut App) -> bool {
    crate::task_dialog::hosted::open_call_window(url, WEB_CALL_TITLE.to_owned(), cx)
}

#[cfg(not(windows))]
fn open_web_call(url: String, cx: &mut App) -> bool {
    let _ = WEB_CALL_TITLE;
    cx.open_url(&url);
    true
}

#[cfg(test)]
mod tests {
    use calling::MeetingTarget;

    use super::{chat_web_url, meeting_web_url};

    #[test]
    fn a_meeting_opens_the_teams_join_link_with_tenant_and_organizer() {
        let target = MeetingTarget {
            thread_id: "19:meeting_abc@thread.v2".to_owned(),
            tenant_id: "tenant-1".to_owned(),
            organizer_id: "organizer-1".to_owned(),
            meeting_data: None,
        };
        assert_eq!(
            meeting_web_url(&target),
            "https://teams.microsoft.com/l/meetup-join/19%3Ameeting_abc%40thread.v2/0?context=%7B%22Tid%22%3A%22tenant-1%22%2C%22Oid%22%3A%22organizer-1%22%7D"
        );
    }

    #[test]
    fn a_chat_opens_its_teams_chat_link() {
        assert_eq!(
            chat_web_url("19:abc@thread.v2"),
            "https://teams.microsoft.com/l/chat/19%3Aabc%40thread.v2/0"
        );
    }
}
