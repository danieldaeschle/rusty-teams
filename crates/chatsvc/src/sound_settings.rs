use serde_json::{Map, Value, json};
use session::{Method, Request, SPACES, Scope, Session};

use crate::error::{Error, Result};
use crate::language_settings::with_headers;
use crate::messages::{MessageTransport, SessionMessageTransport, ensure_success};
use crate::pins::{CsaTransport, DEFAULT_REGION, SessionTransport};
use crate::settings::UserSettings;
use crate::sounds::{AlertSound, Ringtone, RingtoneAssignments, RingtoneCallType};

const SKYPE_SCOPE: &str = "user_impersonation";
const PERSONAL_SETTINGS_KEY: &str = "userPersonalSettings";
const TOAST_SETTINGS: &str = "toastSettings";
const SOUNDS_SETTINGS: &str = "soundsSettings";
const ALERT_SOUND: &str = "alertSound";
const ALERT_SOUND_TYPE: &str = "alertSoundType";
const URGENT_SOUND: &str = "PlayNotificationSoundsForUrgentAndPriority";
const URGENT_SOUND_TYPE: &str = "alertUrgentSoundType";
const CALL_AND_MEETING_SOUND: &str = "incomingCallsAndMeetingStartSound";
const SOUND_WHEN_BUSY: &str = "PlayNotificationSoundsWhenBusyOrInMeetings";
const ALERT_SOUND_ON: &str = "All";
const ON: &str = "On";
const OFF: &str = "Off";
const RINGTONE_ASSIGNMENTS_POINTER: &str =
    "/voiceAdminSettings/value/ringtoneSettings/assignments/ringToneAssignments";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundSettings {
    pub notification_sound: bool,
    pub notification_sound_type: AlertSound,
    pub urgent_sound: bool,
    pub urgent_sound_type: AlertSound,
    pub call_and_meeting_sound: bool,
    pub sound_when_busy: bool,
    pub ringtones: RingtoneAssignments,
}

impl Default for SoundSettings {
    fn default() -> Self {
        SoundSettings {
            notification_sound: true,
            notification_sound_type: AlertSound::Default,
            urgent_sound: true,
            urgent_sound_type: AlertSound::DefaultUrgent,
            call_and_meeting_sound: true,
            sound_when_busy: true,
            ringtones: RingtoneAssignments::default(),
        }
    }
}

impl SoundSettings {
    pub fn personal_part_differs(&self, other: &SoundSettings) -> bool {
        SoundSettings {
            ringtones: other.ringtones,
            ..self.clone()
        } != *other
    }
}

fn personal_settings(properties: &Value) -> Map<String, Value> {
    properties
        .get(PERSONAL_SETTINGS_KEY)
        .and_then(Value::as_str)
        .and_then(|text| serde_json::from_str::<Value>(text).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

fn text_at<'a>(settings: &'a Map<String, Value>, group: &str, field: &str) -> Option<&'a str> {
    settings.get(group)?.get(field)?.as_str()
}

pub fn parse_sound_settings(properties: &Value, aggregate: &Value) -> SoundSettings {
    let settings = personal_settings(properties);
    let defaults = SoundSettings::default();
    let switch = |group: &str, field: &str, fallback: bool| {
        text_at(&settings, group, field).map_or(fallback, |value| value != OFF)
    };
    let sound = |field: &str, fallback: AlertSound| {
        text_at(&settings, TOAST_SETTINGS, field)
            .and_then(AlertSound::from_name)
            .unwrap_or(fallback)
    };
    SoundSettings {
        notification_sound: switch(SOUNDS_SETTINGS, ALERT_SOUND, defaults.notification_sound),
        notification_sound_type: sound(ALERT_SOUND_TYPE, defaults.notification_sound_type),
        urgent_sound: switch(TOAST_SETTINGS, URGENT_SOUND, defaults.urgent_sound),
        urgent_sound_type: sound(URGENT_SOUND_TYPE, defaults.urgent_sound_type),
        call_and_meeting_sound: switch(
            SOUNDS_SETTINGS,
            CALL_AND_MEETING_SOUND,
            defaults.call_and_meeting_sound,
        ),
        sound_when_busy: switch(TOAST_SETTINGS, SOUND_WHEN_BUSY, defaults.sound_when_busy),
        ringtones: parse_ringtones(aggregate),
    }
}

pub fn parse_ringtones(aggregate: &Value) -> RingtoneAssignments {
    let mut assignments = RingtoneAssignments::default();
    let entries = aggregate
        .pointer(RINGTONE_ASSIGNMENTS_POINTER)
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    for entry in entries {
        let call_type = entry
            .get("type")
            .and_then(Value::as_str)
            .and_then(RingtoneCallType::from_name);
        let ringtone = entry
            .get("value")
            .and_then(Value::as_str)
            .and_then(Ringtone::from_name);
        if let (Some(call_type), Some(ringtone)) = (call_type, ringtone) {
            assignments.set(call_type, ringtone);
        }
    }
    assignments
}

fn switch_value(on: bool) -> &'static str {
    if on { ON } else { OFF }
}

/// Writes only the fields that changed, so values rusty-teams does not know (tactile sounds) survive.
pub fn merge_sound_settings(
    properties: &Value,
    previous: &SoundSettings,
    updated: &SoundSettings,
) -> String {
    let mut settings = personal_settings(properties);
    let mut changes: Vec<(&str, &str, &str)> = Vec::new();
    if previous.notification_sound != updated.notification_sound {
        let value = if updated.notification_sound {
            ALERT_SOUND_ON
        } else {
            OFF
        };
        changes.push((SOUNDS_SETTINGS, ALERT_SOUND, value));
    }
    if previous.notification_sound_type != updated.notification_sound_type {
        changes.push((
            TOAST_SETTINGS,
            ALERT_SOUND_TYPE,
            updated.notification_sound_type.name(),
        ));
    }
    if previous.urgent_sound != updated.urgent_sound {
        changes.push((
            TOAST_SETTINGS,
            URGENT_SOUND,
            switch_value(updated.urgent_sound),
        ));
    }
    if previous.urgent_sound_type != updated.urgent_sound_type {
        changes.push((
            TOAST_SETTINGS,
            URGENT_SOUND_TYPE,
            updated.urgent_sound_type.name(),
        ));
    }
    if previous.call_and_meeting_sound != updated.call_and_meeting_sound {
        changes.push((
            SOUNDS_SETTINGS,
            CALL_AND_MEETING_SOUND,
            switch_value(updated.call_and_meeting_sound),
        ));
    }
    if previous.sound_when_busy != updated.sound_when_busy {
        changes.push((
            TOAST_SETTINGS,
            SOUND_WHEN_BUSY,
            switch_value(updated.sound_when_busy),
        ));
    }
    for (group, field, value) in changes {
        let entry = settings
            .entry(group)
            .or_insert_with(|| Value::Object(Map::new()));
        if !entry.is_object() {
            *entry = Value::Object(Map::new());
        }
        if let Some(object) = entry.as_object_mut() {
            object.insert(field.to_owned(), Value::String(value.to_owned()));
        }
    }
    Value::Object(settings).to_string()
}

pub fn ringtone_body(assignments: &RingtoneAssignments) -> Value {
    let entries: Vec<Value> = assignments
        .custom()
        .into_iter()
        .map(|(call_type, ringtone)| {
            json!({"type": call_type.name(), "value": ringtone.name(), "associatedEntityId": ""})
        })
        .collect();
    json!({"assignments": {"ringToneAssignments": entries}})
}

pub fn aggregate_read_body() -> Value {
    json!({"voiceAdminSettings": true})
}

pub struct SoundSettingsClient<
    P: MessageTransport = SessionMessageTransport,
    M: CsaTransport = SessionTransport,
> {
    properties: UserSettings<P>,
    middle_tier: M,
    middle_tier_url: String,
}

impl SoundSettingsClient {
    pub fn new(session: &Session) -> Self {
        SoundSettingsClient::with_transports(
            UserSettings::new(session),
            SessionTransport::with_scope(session, Scope::new(SPACES, SKYPE_SCOPE)),
            DEFAULT_REGION,
        )
    }
}

impl<P: MessageTransport, M: CsaTransport> SoundSettingsClient<P, M> {
    pub fn with_transports(properties: UserSettings<P>, middle_tier: M, region: &str) -> Self {
        SoundSettingsClient {
            properties,
            middle_tier,
            middle_tier_url: format!("https://teams.cloud.microsoft/api/mt/{region}/beta"),
        }
    }

    pub async fn read(&self) -> Result<SoundSettings> {
        let properties = self.properties.properties().await?;
        let url = format!("{}/users/useraggregatesettings", self.middle_tier_url);
        let answer = self
            .middle_tier
            .send(with_headers(Request::with_body(
                Method::Post,
                url,
                aggregate_read_body(),
            )))
            .await?;
        ensure_success(&answer)?;
        if answer.body.get("voiceAdminSettings").is_none() {
            return Err(Error::UnexpectedAnswer("no voice settings".into()));
        }
        Ok(parse_sound_settings(&properties, &answer.body))
    }

    pub async fn write(
        &self,
        user_mri: &str,
        previous: &SoundSettings,
        updated: &SoundSettings,
    ) -> Result<()> {
        if previous.personal_part_differs(updated) {
            let properties = self.properties.properties().await?;
            let merged = merge_sound_settings(&properties, previous, updated);
            self.properties
                .set_property(PERSONAL_SETTINGS_KEY, Value::String(merged))
                .await?;
        }
        if previous.ringtones != updated.ringtones {
            let url = format!(
                "{}/usersettings/{user_mri}/ringtonesettings",
                self.middle_tier_url
            );
            let answer = self
                .middle_tier
                .send(with_headers(Request::with_body(
                    Method::Post,
                    url,
                    ringtone_body(&updated.ringtones),
                )))
                .await?;
            ensure_success(&answer)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn properties(personal: Value) -> Value {
        json!({"other": "kept", PERSONAL_SETTINGS_KEY: personal.to_string()})
    }

    fn aggregate(assignments: Value) -> Value {
        json!({"voiceAdminSettings": {"value": {"ringtoneSettings": {"assignments": {"ringToneAssignments": assignments}}}}})
    }

    #[test]
    fn missing_settings_give_the_teams_defaults() {
        assert_eq!(
            parse_sound_settings(&json!({}), &json!({})),
            SoundSettings::default()
        );
    }

    #[test]
    fn reads_the_account_values_teams_writes() {
        let personal = json!({
            "toastSettings": {
                "alertSoundType": "Tap",
                "alertUrgentSoundType": "Alarm",
                "PlayNotificationSoundsForUrgentAndPriority": "Off",
                "PlayNotificationSoundsWhenBusyOrInMeetings": "Off"
            },
            "soundsSettings": {"alertSound": "Off", "incomingCallsAndMeetingStartSound": "Off"}
        });
        let parsed = parse_sound_settings(
            &properties(personal),
            &aggregate(json!([
                {"type": "Regular", "value": "Bop", "associatedEntityId": ""},
                {"type": "Delegate", "value": "plink"},
                {"type": "Unknown", "value": "Bop"},
                {"type": "CallGroup", "value": "Chime"}
            ])),
        );
        assert_eq!(
            parsed,
            SoundSettings {
                notification_sound: false,
                notification_sound_type: AlertSound::Tap,
                urgent_sound: false,
                urgent_sound_type: AlertSound::Alarm,
                call_and_meeting_sound: false,
                sound_when_busy: false,
                ringtones: RingtoneAssignments {
                    regular: Ringtone::Bop,
                    delegate: Ringtone::Plink,
                    ..Default::default()
                },
            }
        );
    }

    #[test]
    fn an_unknown_sound_falls_back_to_the_default() {
        let personal = json!({"toastSettings": {"alertSoundType": "TACTILE_DEFAULT"}, "soundsSettings": {"alertSound": "All"}});
        let parsed = parse_sound_settings(&properties(personal), &json!({}));
        assert!(parsed.notification_sound);
        assert_eq!(parsed.notification_sound_type, AlertSound::Default);
    }

    #[test]
    fn merge_writes_only_the_changed_fields_and_keeps_the_rest() {
        let personal = json!({
            "toastSettings": {"alertSoundType": "TACTILE_DEFAULT", "Chat": true},
            "soundsSettings": {"alertSound": "All"},
            "emailSettings": {"EmailDigestDelay": "Daily"}
        });
        let previous = SoundSettings::default();
        let updated = SoundSettings {
            notification_sound: false,
            urgent_sound_type: AlertSound::Escalate,
            sound_when_busy: false,
            ..previous.clone()
        };
        let merged: Value = serde_json::from_str(&merge_sound_settings(
            &properties(personal),
            &previous,
            &updated,
        ))
        .unwrap();
        assert_eq!(
            merged,
            json!({
                "toastSettings": {
                    "alertSoundType": "TACTILE_DEFAULT",
                    "Chat": true,
                    "alertUrgentSoundType": "Escalate",
                    "PlayNotificationSoundsWhenBusyOrInMeetings": "Off"
                },
                "soundsSettings": {"alertSound": "Off"},
                "emailSettings": {"EmailDigestDelay": "Daily"}
            })
        );
    }

    #[test]
    fn turning_the_notification_sound_on_writes_all_like_teams() {
        let previous = SoundSettings {
            notification_sound: false,
            ..Default::default()
        };
        let merged: Value = serde_json::from_str(&merge_sound_settings(
            &json!({}),
            &previous,
            &SoundSettings::default(),
        ))
        .unwrap();
        assert_eq!(merged, json!({"soundsSettings": {"alertSound": "All"}}));
    }

    #[test]
    fn the_ringtone_body_lists_only_custom_ringtones() {
        let assignments = RingtoneAssignments {
            call_group: Ringtone::WishGranted,
            ..Default::default()
        };
        assert_eq!(
            ringtone_body(&assignments),
            json!({"assignments": {"ringToneAssignments": [
                {"type": "CallGroup", "value": "WishGranted", "associatedEntityId": ""}
            ]}})
        );
        assert_eq!(
            ringtone_body(&RingtoneAssignments::default()),
            json!({"assignments": {"ringToneAssignments": []}})
        );
    }

    #[test]
    fn only_a_ringtone_change_skips_the_personal_settings() {
        let previous = SoundSettings::default();
        let mut updated = previous.clone();
        updated.ringtones.regular = Ringtone::Bop;
        assert!(!previous.personal_part_differs(&updated));
        updated.call_and_meeting_sound = false;
        assert!(previous.personal_part_differs(&updated));
    }
}
