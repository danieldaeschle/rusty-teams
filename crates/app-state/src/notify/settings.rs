use store::Store;

use super::sounds::{AlertSound, Ringtone, RingtoneAssignments, RingtoneCallType};
use super::rules::{Corner, Settings};

const META_KEY: &str = "notifications.settings";
const RINGTONE_PREFIX: &str = "ringtone.";

pub fn load(store: &Store) -> Settings {
    store
        .meta(META_KEY)
        .ok()
        .flatten()
        .map(|text| parse(&text))
        .unwrap_or_default()
}

pub fn save(store: &Store, settings: &Settings) {
    let _ = store.set_meta(META_KEY, &serialize(settings));
}

fn serialize(settings: &Settings) -> String {
    let sounds = &settings.sounds;
    let ringtones = RingtoneCallType::ALL.map(|call_type| {
        (
            format!("{RINGTONE_PREFIX}{}", call_type.name()),
            sounds.ringtones.get(call_type).name().to_owned(),
        )
    });
    [
        ("sound".to_owned(), sounds.notification_sound.to_string()),
        ("sound_type".to_owned(), sounds.notification_sound_type.name().to_owned()),
        ("urgent_sound".to_owned(), sounds.urgent_sound.to_string()),
        ("urgent_sound_type".to_owned(), sounds.urgent_sound_type.name().to_owned()),
        ("call_sound".to_owned(), sounds.call_and_meeting_sound.to_string()),
        ("sound_when_busy".to_owned(), sounds.sound_when_busy.to_string()),
        ("mentions_only".to_owned(), settings.mentions_only.to_string()),
        ("preview".to_owned(), settings.preview.to_string()),
        ("corner".to_owned(), settings.corner.key().to_owned()),
        ("taskbar_flash".to_owned(), settings.flash.to_string()),
        ("close_to_tray".to_owned(), settings.close_to_tray.to_string()),
        ("do_not_disturb".to_owned(), settings.do_not_disturb.to_string()),
        ("mute_in_calls".to_owned(), settings.mute_in_calls.to_string()),
    ]
    .into_iter()
    .chain(ringtones)
    .map(|(key, value)| format!("{key}={value}"))
    .collect::<Vec<_>>()
    .join("\n")
}

fn parse(text: &str) -> Settings {
    let mut settings = Settings::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let flag = value == "true";
        let sounds = &mut settings.sounds;
        match key {
            "sound" => sounds.notification_sound = flag,
            "sound_type" => set_alert_sound(&mut sounds.notification_sound_type, value),
            "urgent_sound" => sounds.urgent_sound = flag,
            "urgent_sound_type" => set_alert_sound(&mut sounds.urgent_sound_type, value),
            "call_sound" => sounds.call_and_meeting_sound = flag,
            "sound_when_busy" => sounds.sound_when_busy = flag,
            "mentions_only" => settings.mentions_only = flag,
            "preview" => settings.preview = flag,
            "corner" => settings.corner = Corner::from_key(value).unwrap_or_default(),
            "taskbar_flash" => settings.flash = flag,
            "close_to_tray" => settings.close_to_tray = flag,
            "do_not_disturb" => settings.do_not_disturb = flag,
            "mute_in_calls" => settings.mute_in_calls = flag,
            _ => set_ringtone(&mut sounds.ringtones, key, value),
        }
    }
    settings
}

fn set_alert_sound(target: &mut AlertSound, value: &str) {
    if let Some(sound) = AlertSound::from_name(value) {
        *target = sound;
    }
}

fn set_ringtone(ringtones: &mut RingtoneAssignments, key: &str, value: &str) {
    let call_type = key
        .strip_prefix(RINGTONE_PREFIX)
        .and_then(RingtoneCallType::from_name);
    if let (Some(call_type), Some(ringtone)) = (call_type, Ringtone::from_name(value)) {
        ringtones.set(call_type, ringtone);
    }
}

#[cfg(test)]
mod tests {
    use super::super::sound_settings::SoundSettings;
    use super::*;

    #[test]
    fn round_trips_every_field() {
        let settings = Settings {
            mentions_only: true,
            preview: false,
            corner: Corner::TopLeft,
            flash: false,
            close_to_tray: false,
            do_not_disturb: true,
            mute_in_calls: true,
            sounds: SoundSettings {
                notification_sound: false,
                notification_sound_type: AlertSound::Ping,
                urgent_sound: false,
                urgent_sound_type: AlertSound::Alarm,
                call_and_meeting_sound: false,
                sound_when_busy: false,
                ringtones: RingtoneAssignments {
                    regular: Ringtone::Bop,
                    delegate: Ringtone::WishGranted,
                    ..Default::default()
                },
            },
        };
        assert_eq!(parse(&serialize(&settings)), settings);
    }

    #[test]
    fn old_flash_key_is_ignored() {
        assert!(parse("flash=false").flash);
        assert!(!parse("taskbar_flash=false").flash);
    }

    #[test]
    fn the_old_synthesized_ringtone_key_is_ignored() {
        assert_eq!(parse("ringtone=chime"), Settings::default());
    }

    #[test]
    fn empty_or_garbage_gives_defaults() {
        assert_eq!(parse(""), Settings::default());
        assert_eq!(parse("junk\ncorner=nowhere"), Settings::default());
    }

    #[test]
    fn persists_in_store() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(load(&store), Settings::default());
        let mut changed = Settings::default();
        changed.sounds.notification_sound = false;
        save(&store, &changed);
        assert_eq!(load(&store), changed);
    }
}
