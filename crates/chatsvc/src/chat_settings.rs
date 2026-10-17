use serde_json::{Map, Value, json};
use session::Session;

use crate::error::Result;
use crate::messages::{MessageTransport, SessionMessageTransport};
use crate::settings::UserSettings;

const READ_RECEIPTS_KEY: &str = "readReceiptsEnabled";
const MESSAGE_DENSITY_KEY: &str = "messageDensity";
const COMPOSE_SETTINGS_KEY: &str = "composeUserSettings";
const PERSONAL_SETTINGS_KEY: &str = "userPersonalSettings";
const ENTER_KEY_FIELD: &str = "enterKeyBehavior";
const MESSAGING_SETTINGS: &str = "messagingSettings";
const UP_ARROW_FIELD: &str = "upArrowKeyBehavior";
const TOAST_SETTINGS: &str = "toastSettings";
const ACTIVITY_SETTINGS: &str = "activityGenerationSettings";
const EMAIL_SETTINGS: &str = "emailSettings";
const CHAT_SETTINGS: &str = "chatSettings";
const SELECTED_PRESET_FIELD: &str = "selectedPreset";
const MEETING_CHAT_FIELD: &str = "meetingChatNotification";
const SHOW_PREVIEW_FIELD: &str = "ShowPreviewInToasts";
const CHAT_MESSAGES_FIELD: &str = "Chat";
const PERSONAL_MENTIONS_FIELD: &str = "MentionInChat_Person_v2";
const EVERYONE_MENTIONS_FIELD: &str = "MentionInChat_Everyone";
const LIKES_FIELD: &str = "LikeInChat_v2";
const ACCOUNT_KEYS: [&str; 4] = [
    READ_RECEIPTS_KEY,
    MESSAGE_DENSITY_KEY,
    COMPOSE_SETTINGS_KEY,
    PERSONAL_SETTINGS_KEY,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MessageDensity {
    #[default]
    Comfy,
    Compact,
}

impl MessageDensity {
    pub const ALL: [MessageDensity; 2] = [MessageDensity::Comfy, MessageDensity::Compact];

    pub fn property_value(self) -> &'static str {
        match self {
            MessageDensity::Comfy => "COMFY",
            MessageDensity::Compact => "COMPACT",
        }
    }

    fn from_property(value: &str) -> Option<Self> {
        MessageDensity::ALL
            .into_iter()
            .find(|density| density.property_value() == value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EnterKeyBehavior {
    #[default]
    Send,
    NewLine,
}

impl EnterKeyBehavior {
    pub const ALL: [EnterKeyBehavior; 2] = [EnterKeyBehavior::Send, EnterKeyBehavior::NewLine];

    pub fn property_value(self) -> &'static str {
        match self {
            EnterKeyBehavior::Send => "Default",
            EnterKeyBehavior::NewLine => "CreateNewLine",
        }
    }

    fn from_property(value: &str) -> Option<Self> {
        EnterKeyBehavior::ALL
            .into_iter()
            .find(|behavior| behavior.property_value() == value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UpArrowBehavior {
    NavigateToMessages,
    #[default]
    EditLastMessage,
    StayInCompose,
}

impl UpArrowBehavior {
    pub const ALL: [UpArrowBehavior; 3] = [
        UpArrowBehavior::NavigateToMessages,
        UpArrowBehavior::EditLastMessage,
        UpArrowBehavior::StayInCompose,
    ];

    pub fn property_value(self) -> &'static str {
        match self {
            UpArrowBehavior::NavigateToMessages => "NAVIGATE_TO_MESSAGES",
            UpArrowBehavior::EditLastMessage => "EDIT_LAST_MESSAGE",
            UpArrowBehavior::StayInCompose => "STAY_IN_COMPOSE",
        }
    }

    fn from_property(value: &str) -> Option<Self> {
        UpArrowBehavior::ALL
            .into_iter()
            .find(|behavior| behavior.property_value() == value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NotificationPreset {
    AllActivity,
    MentionsAndReplies,
    #[default]
    Customised,
}

impl NotificationPreset {
    pub const ALL: [NotificationPreset; 3] = [
        NotificationPreset::AllActivity,
        NotificationPreset::MentionsAndReplies,
        NotificationPreset::Customised,
    ];

    pub fn property_value(self) -> i64 {
        match self {
            NotificationPreset::AllActivity => 1,
            NotificationPreset::MentionsAndReplies => 2,
            NotificationPreset::Customised => 3,
        }
    }

    fn from_property(value: &Value) -> Option<Self> {
        let number = value
            .as_i64()
            .or_else(|| value.as_str().and_then(|text| text.parse().ok()))?;
        NotificationPreset::ALL
            .into_iter()
            .find(|preset| preset.property_value() == number)
    }
}

/// Teams' "Show in Activity and banner" / "Show in Activity" / "Off" choice, stored as toast, email and activity flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActivityAlert {
    #[default]
    BannerAndFeed,
    OnlyFeed,
    OnlyBanner,
    Off,
}

impl ActivityAlert {
    fn flags(self) -> (bool, bool, bool) {
        match self {
            ActivityAlert::Off => (false, false, false),
            ActivityAlert::OnlyFeed => (false, false, true),
            ActivityAlert::OnlyBanner => (true, false, true),
            ActivityAlert::BannerAndFeed => (true, true, true),
        }
    }

    pub fn shows_banner(self) -> bool {
        matches!(
            self,
            ActivityAlert::BannerAndFeed | ActivityAlert::OnlyBanner
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MeetingChatNotifications {
    Mute,
    Unmute,
    #[default]
    MuteUntilJoin,
}

impl MeetingChatNotifications {
    pub const ALL: [MeetingChatNotifications; 3] = [
        MeetingChatNotifications::Mute,
        MeetingChatNotifications::Unmute,
        MeetingChatNotifications::MuteUntilJoin,
    ];

    pub fn property_value(self) -> &'static str {
        match self {
            MeetingChatNotifications::Mute => "MeetingsManuallyUnmuted",
            MeetingChatNotifications::Unmute => "MeetingsUserIsInvitedTo",
            MeetingChatNotifications::MuteUntilJoin => "MeetingsUserParticipatesIn",
        }
    }

    fn from_property(value: &str) -> Option<Self> {
        MeetingChatNotifications::ALL
            .into_iter()
            .find(|choice| choice.property_value() == value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationAccountSettings {
    pub preset: NotificationPreset,
    pub chat_messages: ActivityAlert,
    pub personal_mentions: ActivityAlert,
    pub everyone_mentions: ActivityAlert,
    pub likes: ActivityAlert,
    pub show_preview: bool,
    pub meeting_chats: MeetingChatNotifications,
}

impl Default for NotificationAccountSettings {
    fn default() -> Self {
        NotificationAccountSettings {
            preset: NotificationPreset::default(),
            chat_messages: ActivityAlert::OnlyBanner,
            personal_mentions: ActivityAlert::BannerAndFeed,
            everyone_mentions: ActivityAlert::BannerAndFeed,
            likes: ActivityAlert::BannerAndFeed,
            show_preview: true,
            meeting_chats: MeetingChatNotifications::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChatAccountSettings {
    pub read_receipts: bool,
    pub message_density: MessageDensity,
    pub enter_key: EnterKeyBehavior,
    pub up_arrow: UpArrowBehavior,
    pub notifications: NotificationAccountSettings,
}

impl Default for ChatAccountSettings {
    fn default() -> Self {
        ChatAccountSettings {
            read_receipts: true,
            message_density: MessageDensity::default(),
            enter_key: EnterKeyBehavior::default(),
            up_arrow: UpArrowBehavior::default(),
            notifications: NotificationAccountSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatSettingChange {
    ReadReceipts(bool),
    MessageDensity(MessageDensity),
    EnterKey(EnterKeyBehavior),
    UpArrow(UpArrowBehavior),
    Preset(NotificationPreset),
    ChatMessages(ActivityAlert),
    PersonalMentions(ActivityAlert),
    EveryoneMentions(ActivityAlert),
    Likes(ActivityAlert),
    ShowPreview(bool),
    MeetingChats(MeetingChatNotifications),
}

/// The account properties Teams keeps these settings in, as last read from `users/ME/properties`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChatSettingsDocument {
    properties: Map<String, Value>,
}

impl ChatSettingsDocument {
    pub fn from_properties(properties: &Value) -> Self {
        let properties = ACCOUNT_KEYS
            .iter()
            .filter_map(|key| Some(((*key).to_owned(), properties.get(*key)?.clone())))
            .collect();
        ChatSettingsDocument { properties }
    }

    pub fn from_text(text: &str) -> Self {
        serde_json::from_str::<Value>(text)
            .map(|value| ChatSettingsDocument::from_properties(&value))
            .unwrap_or_default()
    }

    pub fn to_text(&self) -> String {
        Value::Object(self.properties.clone()).to_string()
    }

    pub fn settings(&self) -> ChatAccountSettings {
        let defaults = ChatAccountSettings::default();
        let personal = self.json_property(PERSONAL_SETTINGS_KEY);
        let compose = self.json_property(COMPOSE_SETTINGS_KEY);
        ChatAccountSettings {
            read_receipts: self
                .properties
                .get(READ_RECEIPTS_KEY)
                .and_then(flag)
                .unwrap_or(defaults.read_receipts),
            message_density: self
                .properties
                .get(MESSAGE_DENSITY_KEY)
                .and_then(Value::as_str)
                .and_then(MessageDensity::from_property)
                .unwrap_or(defaults.message_density),
            enter_key: text_at(&compose, &[ENTER_KEY_FIELD])
                .and_then(EnterKeyBehavior::from_property)
                .unwrap_or(defaults.enter_key),
            up_arrow: text_at(&personal, &[MESSAGING_SETTINGS, UP_ARROW_FIELD])
                .and_then(UpArrowBehavior::from_property)
                .unwrap_or(defaults.up_arrow),
            notifications: notification_settings(&personal),
        }
    }

    /// Applies the change and returns the property write Teams makes for it: name and value.
    pub fn apply(&mut self, change: ChatSettingChange) -> (&'static str, Value) {
        let (name, value) = match change {
            ChatSettingChange::ReadReceipts(enabled) => {
                (READ_RECEIPTS_KEY, Value::String(enabled.to_string()))
            }
            ChatSettingChange::MessageDensity(density) => (
                MESSAGE_DENSITY_KEY,
                Value::String(density.property_value().to_owned()),
            ),
            ChatSettingChange::EnterKey(behavior) => {
                let mut compose = self.json_property(COMPOSE_SETTINGS_KEY);
                compose.insert(ENTER_KEY_FIELD.to_owned(), json!(behavior.property_value()));
                (
                    COMPOSE_SETTINGS_KEY,
                    Value::String(Value::Object(compose).to_string()),
                )
            }
            personal_change => {
                let mut personal = self.json_property(PERSONAL_SETTINGS_KEY);
                apply_personal(&mut personal, personal_change);
                (
                    PERSONAL_SETTINGS_KEY,
                    Value::String(Value::Object(personal).to_string()),
                )
            }
        };
        self.properties.insert(name.to_owned(), value.clone());
        (name, value)
    }

    fn json_property(&self, key: &str) -> Map<String, Value> {
        match self.properties.get(key) {
            Some(Value::String(text)) => serde_json::from_str::<Value>(text)
                .ok()
                .and_then(|value| value.as_object().cloned())
                .unwrap_or_default(),
            Some(Value::Object(object)) => object.clone(),
            _ => Map::new(),
        }
    }
}

fn flag(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(flag) => Some(*flag),
        Value::String(text) => Some(!text.trim().eq_ignore_ascii_case("false")),
        _ => None,
    }
}

fn value_at<'a>(object: &'a Map<String, Value>, path: &[&str]) -> Option<&'a Value> {
    let (first, rest) = path.split_first()?;
    rest.iter()
        .try_fold(object.get(*first)?, |value, field| value.get(*field))
}

fn text_at<'a>(object: &'a Map<String, Value>, path: &[&str]) -> Option<&'a str> {
    value_at(object, path)?.as_str()
}

fn flag_at(object: &Map<String, Value>, group: &str, field: &str) -> Option<bool> {
    value_at(object, &[group, field]).and_then(flag)
}

fn activity_alert(
    personal: &Map<String, Value>,
    field: &str,
    fallback: ActivityAlert,
) -> ActivityAlert {
    let (fallback_toast, _, fallback_activity) = fallback.flags();
    let toast = flag_at(personal, TOAST_SETTINGS, field).unwrap_or(fallback_toast);
    let activity = flag_at(personal, ACTIVITY_SETTINGS, field).unwrap_or(fallback_activity);
    match (activity, toast) {
        (true, true) => ActivityAlert::BannerAndFeed,
        (true, false) => ActivityAlert::OnlyFeed,
        (false, true) => ActivityAlert::OnlyBanner,
        (false, false) => ActivityAlert::Off,
    }
}

fn chat_messages_alert(personal: &Map<String, Value>) -> ActivityAlert {
    match flag_at(personal, TOAST_SETTINGS, CHAT_MESSAGES_FIELD) {
        Some(false) => ActivityAlert::Off,
        _ => ActivityAlert::OnlyBanner,
    }
}

fn notification_settings(personal: &Map<String, Value>) -> NotificationAccountSettings {
    let defaults = NotificationAccountSettings::default();
    NotificationAccountSettings {
        preset: personal
            .get(SELECTED_PRESET_FIELD)
            .and_then(NotificationPreset::from_property)
            .unwrap_or(defaults.preset),
        chat_messages: chat_messages_alert(personal),
        personal_mentions: activity_alert(
            personal,
            PERSONAL_MENTIONS_FIELD,
            defaults.personal_mentions,
        ),
        everyone_mentions: activity_alert(
            personal,
            EVERYONE_MENTIONS_FIELD,
            defaults.everyone_mentions,
        ),
        likes: activity_alert(personal, LIKES_FIELD, defaults.likes),
        show_preview: text_at(personal, &[TOAST_SETTINGS, SHOW_PREVIEW_FIELD])
            .map_or(defaults.show_preview, |value| value != "Off"),
        meeting_chats: text_at(personal, &[CHAT_SETTINGS, MEETING_CHAT_FIELD])
            .and_then(MeetingChatNotifications::from_property)
            .unwrap_or(defaults.meeting_chats),
    }
}

fn group_mut<'a>(personal: &'a mut Map<String, Value>, group: &str) -> &'a mut Map<String, Value> {
    let entry = personal
        .entry(group.to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    entry.as_object_mut().expect("group is an object")
}

fn set_field(personal: &mut Map<String, Value>, group: &str, field: &str, value: Value) {
    group_mut(personal, group).insert(field.to_owned(), value);
}

fn set_alert(personal: &mut Map<String, Value>, field: &str, alert: ActivityAlert) {
    let (toast, email, activity) = alert.flags();
    set_field(personal, TOAST_SETTINGS, field, Value::Bool(toast));
    set_field(personal, EMAIL_SETTINGS, field, Value::Bool(email));
    set_field(personal, ACTIVITY_SETTINGS, field, Value::Bool(activity));
}

fn apply_personal(personal: &mut Map<String, Value>, change: ChatSettingChange) {
    match change {
        ChatSettingChange::UpArrow(behavior) => set_field(
            personal,
            MESSAGING_SETTINGS,
            UP_ARROW_FIELD,
            json!(behavior.property_value()),
        ),
        ChatSettingChange::Preset(preset) => {
            personal.insert(
                SELECTED_PRESET_FIELD.to_owned(),
                json!(preset.property_value()),
            );
        }
        ChatSettingChange::ChatMessages(alert) => set_alert(personal, CHAT_MESSAGES_FIELD, alert),
        ChatSettingChange::PersonalMentions(alert) => {
            set_alert(personal, PERSONAL_MENTIONS_FIELD, alert)
        }
        ChatSettingChange::EveryoneMentions(alert) => {
            set_alert(personal, EVERYONE_MENTIONS_FIELD, alert)
        }
        ChatSettingChange::Likes(alert) => set_alert(personal, LIKES_FIELD, alert),
        ChatSettingChange::ShowPreview(shown) => set_field(
            personal,
            TOAST_SETTINGS,
            SHOW_PREVIEW_FIELD,
            json!(if shown { "On" } else { "Off" }),
        ),
        ChatSettingChange::MeetingChats(choice) => set_field(
            personal,
            CHAT_SETTINGS,
            MEETING_CHAT_FIELD,
            json!(choice.property_value()),
        ),
        ChatSettingChange::ReadReceipts(_)
        | ChatSettingChange::MessageDensity(_)
        | ChatSettingChange::EnterKey(_) => {}
    }
}

pub struct ChatSettingsClient<T: MessageTransport = SessionMessageTransport> {
    settings: UserSettings<T>,
}

impl ChatSettingsClient<SessionMessageTransport> {
    pub fn new(session: &Session) -> Self {
        ChatSettingsClient::with_settings(UserSettings::new(session))
    }
}

impl<T: MessageTransport> ChatSettingsClient<T> {
    pub fn with_settings(settings: UserSettings<T>) -> Self {
        ChatSettingsClient { settings }
    }

    pub async fn read(&self) -> Result<ChatSettingsDocument> {
        Ok(ChatSettingsDocument::from_properties(
            &self.settings.properties().await?,
        ))
    }

    /// Reads the current properties first so fields this client does not know survive the write.
    pub async fn write(&self, change: ChatSettingChange) -> Result<ChatSettingsDocument> {
        let mut document = self.read().await?;
        let (name, value) = document.apply(change);
        self.settings.set_property(name, value).await?;
        Ok(document)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use session::{ApiResponse, Method, Request};

    use super::*;

    struct Canned {
        answers: Mutex<VecDeque<(u16, Value)>>,
        requests: Mutex<Vec<Request>>,
    }

    impl MessageTransport for &Canned {
        async fn send(&self, request: Request) -> Result<ApiResponse> {
            self.requests.lock().unwrap().push(request);
            let (status, body) = self.answers.lock().unwrap().pop_front().expect("exhausted");
            Ok(ApiResponse {
                status,
                body,
                retry_after: None,
            })
        }
    }

    fn parsed(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn missing_properties_give_the_teams_defaults() {
        let settings = ChatSettingsDocument::from_properties(&json!({})).settings();
        assert_eq!(settings, ChatAccountSettings::default());
        assert!(settings.read_receipts);
        assert_eq!(settings.message_density, MessageDensity::Comfy);
        assert_eq!(settings.enter_key, EnterKeyBehavior::Send);
        assert_eq!(settings.up_arrow, UpArrowBehavior::EditLastMessage);
        assert_eq!(
            settings.notifications.preset,
            NotificationPreset::Customised
        );
        assert_eq!(
            settings.notifications.meeting_chats,
            MeetingChatNotifications::MuteUntilJoin
        );
    }

    #[test]
    fn reads_every_setting_from_the_teams_properties() {
        let personal = json!({
            "messagingSettings": {"upArrowKeyBehavior": "STAY_IN_COMPOSE"},
            "selectedPreset": 2,
            "toastSettings": {"Chat": false, "MentionInChat_Person_v2": false, "LikeInChat_v2": true, "ShowPreviewInToasts": "Off"},
            "activityGenerationSettings": {"MentionInChat_Person_v2": true, "LikeInChat_v2": false, "MentionInChat_Everyone": false},
            "chatSettings": {"meetingChatNotification": "MeetingsUserIsInvitedTo"},
        });
        let document = ChatSettingsDocument::from_properties(&json!({
            "readReceiptsEnabled": "false",
            "messageDensity": "COMPACT",
            "composeUserSettings": "{\"enterKeyBehavior\":\"CreateNewLine\"}",
            "userPersonalSettings": personal.to_string(),
            "unrelated": "dropped",
        }));
        let settings = document.settings();
        assert!(!settings.read_receipts);
        assert_eq!(settings.message_density, MessageDensity::Compact);
        assert_eq!(settings.enter_key, EnterKeyBehavior::NewLine);
        assert_eq!(settings.up_arrow, UpArrowBehavior::StayInCompose);
        let notifications = settings.notifications;
        assert_eq!(notifications.preset, NotificationPreset::MentionsAndReplies);
        assert_eq!(notifications.chat_messages, ActivityAlert::Off);
        assert_eq!(notifications.personal_mentions, ActivityAlert::OnlyFeed);
        assert_eq!(notifications.likes, ActivityAlert::OnlyBanner);
        assert_eq!(notifications.everyone_mentions, ActivityAlert::OnlyBanner);
        assert!(!notifications.show_preview);
        assert_eq!(
            notifications.meeting_chats,
            MeetingChatNotifications::Unmute
        );
        assert!(!document.to_text().contains("unrelated"));
    }

    #[test]
    fn read_receipts_follow_teams_parsing() {
        let read = |value: Value| {
            ChatSettingsDocument::from_properties(&json!({"readReceiptsEnabled": value}))
                .settings()
                .read_receipts
        };
        assert!(read(json!("true")));
        assert!(!read(json!(" False ")));
        assert!(!read(json!(false)));
        assert!(read(json!("anything")));
    }

    #[test]
    fn plain_properties_are_written_as_teams_strings() {
        let mut document = ChatSettingsDocument::default();
        assert_eq!(
            document.apply(ChatSettingChange::ReadReceipts(false)),
            ("readReceiptsEnabled", json!("false"))
        );
        assert_eq!(
            document.apply(ChatSettingChange::MessageDensity(MessageDensity::Compact)),
            ("messageDensity", json!("COMPACT"))
        );
        let settings = document.settings();
        assert!(!settings.read_receipts);
        assert_eq!(settings.message_density, MessageDensity::Compact);
    }

    #[test]
    fn enter_key_keeps_other_compose_fields() {
        let mut document = ChatSettingsDocument::from_properties(&json!({
            "composeUserSettings": "{\"enterKeyBehavior\":\"Default\",\"future\":1}"
        }));
        let (name, value) = document.apply(ChatSettingChange::EnterKey(EnterKeyBehavior::NewLine));
        assert_eq!(name, "composeUserSettings");
        assert_eq!(
            parsed(value.as_str().unwrap()),
            json!({"enterKeyBehavior": "CreateNewLine", "future": 1})
        );
    }

    #[test]
    fn personal_settings_changes_keep_unknown_fields() {
        let mut document = ChatSettingsDocument::from_properties(&json!({
            "userPersonalSettings": "{\"soundsSettings\":{\"alertSound\":\"All\"},\"messagingSettings\":{\"shouldShowDiscoverApp\":true}}"
        }));
        document.apply(ChatSettingChange::UpArrow(
            UpArrowBehavior::NavigateToMessages,
        ));
        let (name, value) =
            document.apply(ChatSettingChange::Preset(NotificationPreset::AllActivity));
        assert_eq!(name, "userPersonalSettings");
        assert_eq!(
            parsed(value.as_str().unwrap()),
            json!({
                "soundsSettings": {"alertSound": "All"},
                "messagingSettings": {"shouldShowDiscoverApp": true, "upArrowKeyBehavior": "NAVIGATE_TO_MESSAGES"},
                "selectedPreset": 1,
            })
        );
    }

    #[test]
    fn alerts_write_toast_email_and_activity_like_teams() {
        let mut document = ChatSettingsDocument::default();
        document.apply(ChatSettingChange::ChatMessages(ActivityAlert::Off));
        document.apply(ChatSettingChange::PersonalMentions(ActivityAlert::OnlyFeed));
        let (_, value) = document.apply(ChatSettingChange::Likes(ActivityAlert::BannerAndFeed));
        let personal = parsed(value.as_str().unwrap());
        assert_eq!(
            personal["toastSettings"],
            json!({"Chat": false, "MentionInChat_Person_v2": false, "LikeInChat_v2": true})
        );
        assert_eq!(
            personal["emailSettings"],
            json!({"Chat": false, "MentionInChat_Person_v2": false, "LikeInChat_v2": true})
        );
        assert_eq!(
            personal["activityGenerationSettings"],
            json!({"Chat": false, "MentionInChat_Person_v2": true, "LikeInChat_v2": true})
        );
        let notifications = document.settings().notifications;
        assert_eq!(notifications.chat_messages, ActivityAlert::Off);
        assert_eq!(notifications.personal_mentions, ActivityAlert::OnlyFeed);
        assert_eq!(notifications.likes, ActivityAlert::BannerAndFeed);
    }

    #[test]
    fn preview_and_meeting_chats_round_trip() {
        let mut document = ChatSettingsDocument::default();
        document.apply(ChatSettingChange::ShowPreview(false));
        document.apply(ChatSettingChange::MeetingChats(
            MeetingChatNotifications::Mute,
        ));
        let restored = ChatSettingsDocument::from_text(&document.to_text());
        let notifications = restored.settings().notifications;
        assert!(!notifications.show_preview);
        assert_eq!(notifications.meeting_chats, MeetingChatNotifications::Mute);
        assert_eq!(restored, document);
    }

    #[tokio::test]
    async fn write_reads_then_puts_the_named_property() {
        let transport = Canned {
            answers: Mutex::new(
                vec![
                    (200, json!({"messageDensity": "COMFY", "other": 1})),
                    (200, Value::Null),
                ]
                .into(),
            ),
            requests: Mutex::new(Vec::new()),
        };
        let client =
            ChatSettingsClient::with_settings(UserSettings::with_transport(&transport, "emea"));
        let document = client
            .write(ChatSettingChange::MessageDensity(MessageDensity::Compact))
            .await
            .unwrap();
        assert_eq!(document.settings().message_density, MessageDensity::Compact);
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests[0].method, Method::Get);
        assert_eq!(requests[1].method, Method::Put);
        assert_eq!(
            requests[1].url,
            "https://teams.cloud.microsoft/api/chatsvc/emea/v1/users/ME/properties?name=messageDensity"
        );
        assert_eq!(requests[1].body, Some(json!({"messageDensity": "COMPACT"})));
    }
}
