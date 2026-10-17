use serde_json::{Map, Value};

pub(crate) const PROPERTY_NAME: &str = "conversationNotificationSettings";
const ALL_NEW_POSTS: &str = "allNewPosts";
const DESKTOP: &str = "dskNotif";
const EVERYONE: &str = "everyone";
const ON: &str = "On";
const OFF: &str = "Off";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChatNotificationMode {
    #[default]
    AllNewMessages,
    Mute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DesktopNotifications {
    #[default]
    MatchAllChats,
    On,
    Off,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatNotificationWrite {
    pub alerts: bool,
    pub settings: String,
}

fn stored(raw: Option<&str>) -> Map<String, Value> {
    raw.and_then(|text| serde_json::from_str::<Value>(text).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

pub fn parse_desktop(raw: Option<&str>) -> DesktopNotifications {
    match stored(raw).get(DESKTOP).and_then(Value::as_str) {
        Some(ON) => DesktopNotifications::On,
        Some(OFF) => DesktopNotifications::Off,
        _ => DesktopNotifications::MatchAllChats,
    }
}

/// Teams' granular chat setting: `alerts` plus the merged `conversationNotificationSettings` text.
pub fn chat_notification_write(
    raw: Option<&str>,
    mode: ChatNotificationMode,
    desktop: DesktopNotifications,
) -> ChatNotificationWrite {
    let mut settings = stored(raw);
    let desktop_value = match desktop {
        DesktopNotifications::MatchAllChats => Value::Null,
        DesktopNotifications::On => Value::String(ON.to_owned()),
        DesktopNotifications::Off => Value::String(OFF.to_owned()),
    };
    settings.insert(DESKTOP.to_owned(), desktop_value);
    let alerts = mode == ChatNotificationMode::AllNewMessages;
    settings.insert(
        ALL_NEW_POSTS.to_owned(),
        Value::String(if alerts { ON } else { OFF }.to_owned()),
    );
    if !alerts {
        settings.insert(EVERYONE.to_owned(), Value::String(OFF.to_owned()));
    }
    ChatNotificationWrite {
        alerts,
        settings: Value::Object(settings).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn written(write: &ChatNotificationWrite) -> Value {
        serde_json::from_str(&write.settings).unwrap()
    }

    #[test]
    fn desktop_preference_is_read_from_dsk_notif() {
        assert_eq!(parse_desktop(None), DesktopNotifications::MatchAllChats);
        assert_eq!(
            parse_desktop(Some(r#"{"dskNotif":"Off"}"#)),
            DesktopNotifications::Off
        );
        assert_eq!(
            parse_desktop(Some(r#"{"dskNotif":"On","allNewPosts":"On"}"#)),
            DesktopNotifications::On
        );
        assert_eq!(
            parse_desktop(Some(r#"{"dskNotif":null}"#)),
            DesktopNotifications::MatchAllChats
        );
        assert_eq!(
            parse_desktop(Some("garbage")),
            DesktopNotifications::MatchAllChats
        );
    }

    #[test]
    fn all_new_messages_turns_alerts_on_and_keeps_other_fields() {
        let write = chat_notification_write(
            Some(r#"{"everyone":"On","future":1}"#),
            ChatNotificationMode::AllNewMessages,
            DesktopNotifications::MatchAllChats,
        );
        assert!(write.alerts);
        assert_eq!(
            written(&write),
            json!({"everyone": "On", "future": 1, "dskNotif": null, "allNewPosts": "On"})
        );
    }

    #[test]
    fn mute_turns_alerts_and_everyone_mentions_off() {
        let write = chat_notification_write(
            Some(r#"{"everyone":"On"}"#),
            ChatNotificationMode::Mute,
            DesktopNotifications::Off,
        );
        assert!(!write.alerts);
        assert_eq!(
            written(&write),
            json!({"everyone": "Off", "dskNotif": "Off", "allNewPosts": "Off"})
        );
    }
}
