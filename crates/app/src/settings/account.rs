use gpui_kit::*;
use teams_core::{
    ActivityAlert, ChatSettingChange, EnterKeyBehavior, MeetingChatNotifications, MessageDensity,
    NotificationPreset, UpArrowBehavior,
};

use super::rows::{Choice, control_row, select};
use crate::app_state::AppState;

pub struct Labeled<T: 'static> {
    pub label: &'static str,
    pub value: T,
}

const fn option<T>(label: &'static str, value: T) -> Labeled<T> {
    Labeled { label, value }
}

pub const DENSITY: [Labeled<MessageDensity>; 2] = [
    option("Comfy", MessageDensity::Comfy),
    option("Compact", MessageDensity::Compact),
];

pub const ENTER_KEY: [Labeled<EnterKeyBehavior>; 2] = [
    option("Send the message", EnterKeyBehavior::Send),
    option(
        "Start a new line (to send, select Send or use Ctrl+Enter).",
        EnterKeyBehavior::NewLine,
    ),
];

pub const UP_ARROW: [Labeled<UpArrowBehavior>; 3] = [
    option(
        "Move focus to message list",
        UpArrowBehavior::NavigateToMessages,
    ),
    option("Edit last message", UpArrowBehavior::EditLastMessage),
    option("Stay in compose box", UpArrowBehavior::StayInCompose),
];

pub const PRESETS: [Labeled<NotificationPreset>; 3] = [
    option("All activity", NotificationPreset::AllActivity),
    option("Mentions & replies", NotificationPreset::MentionsAndReplies),
    option("Customised", NotificationPreset::Customised),
];

pub const CHAT_MESSAGES: [Labeled<ActivityAlert>; 2] = [
    option("Show in banner", ActivityAlert::OnlyBanner),
    option("Off", ActivityAlert::Off),
];

pub const MENTIONS: [Labeled<ActivityAlert>; 2] = [
    option("Show in Activity and banner", ActivityAlert::BannerAndFeed),
    option("Show in Activity", ActivityAlert::OnlyFeed),
];

pub const ACTIVITY_OR_OFF: [Labeled<ActivityAlert>; 3] = [
    option("Show in Activity and banner", ActivityAlert::BannerAndFeed),
    option("Show in Activity", ActivityAlert::OnlyFeed),
    option("Off", ActivityAlert::Off),
];

pub const MEETING_CHATS: [Labeled<MeetingChatNotifications>; 3] = [
    option("Mute", MeetingChatNotifications::Mute),
    option("Unmute", MeetingChatNotifications::Unmute),
    option(
        "Mute until I join or send a message",
        MeetingChatNotifications::MuteUntilJoin,
    ),
];

pub fn preset_detail(preset: NotificationPreset) -> &'static str {
    match preset {
        NotificationPreset::AllActivity => "New messages, reactions, and all mentions",
        NotificationPreset::MentionsAndReplies => "Personal mentions and replies to your messages",
        NotificationPreset::Customised => "Your customised settings are active.",
    }
}

pub fn meeting_chats_detail(setting: MeetingChatNotifications) -> &'static str {
    match setting {
        MeetingChatNotifications::Mute => "Don't send notifications for any meetings.",
        MeetingChatNotifications::Unmute => "Send notifications for all meetings.",
        MeetingChatNotifications::MuteUntilJoin => {
            "Only send notifications if I've joined a meeting or sent a message."
        }
    }
}

pub fn label_of<T: PartialEq + Copy>(options: &[Labeled<T>], current: T) -> &'static str {
    options
        .iter()
        .find(|option| option.value == current)
        .map(|option| option.label)
        .unwrap_or_default()
}

pub fn account_select<T: PartialEq + Copy + 'static>(
    app: &Entity<AppState>,
    (id, title, detail): (&'static str, &'static str, &'static str),
    options: &'static [Labeled<T>],
    current: T,
    change: fn(T) -> ChatSettingChange,
) -> Div {
    let choices = options
        .iter()
        .map(|option| {
            let (app, value) = (app.clone(), option.value);
            Choice::new(option.label, value == current, move |cx| {
                app.update(cx, |state, cx| state.save_chat_setting(change(value), cx))
            })
        })
        .collect();
    control_row(
        title,
        detail,
        select(id, label_of(options, current), false, choices),
    )
}

#[cfg(test)]
mod tests {
    use teams_core::{ActivityAlert, MeetingChatNotifications, MessageDensity, UpArrowBehavior};

    use super::{CHAT_MESSAGES, DENSITY, MEETING_CHATS, MENTIONS, UP_ARROW, label_of};

    #[test]
    fn every_value_has_a_teams_label() {
        assert_eq!(label_of(&DENSITY, MessageDensity::Compact), "Compact");
        assert_eq!(
            label_of(&UP_ARROW, UpArrowBehavior::default()),
            "Edit last message"
        );
        assert_eq!(
            label_of(&MEETING_CHATS, MeetingChatNotifications::default()),
            "Mute until I join or send a message"
        );
        assert_eq!(
            label_of(&CHAT_MESSAGES, ActivityAlert::OnlyBanner),
            "Show in banner"
        );
        assert_eq!(label_of(&MENTIONS, ActivityAlert::Off), "");
    }
}
