use store::Store;

use super::rules::{Corner, Settings};

const META_KEY: &str = "notifications.settings";

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
    [
        ("sound", settings.sound.to_string()),
        ("mentions_only", settings.mentions_only.to_string()),
        ("preview", settings.preview.to_string()),
        ("corner", settings.corner.key().to_owned()),
        ("taskbar_flash", settings.flash.to_string()),
        ("close_to_tray", settings.close_to_tray.to_string()),
        ("do_not_disturb", settings.do_not_disturb.to_string()),
    ]
    .iter()
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
        match key {
            "sound" => settings.sound = flag,
            "mentions_only" => settings.mentions_only = flag,
            "preview" => settings.preview = flag,
            "corner" => settings.corner = Corner::from_key(value).unwrap_or_default(),
            "taskbar_flash" => settings.flash = flag,
            "close_to_tray" => settings.close_to_tray = flag,
            "do_not_disturb" => settings.do_not_disturb = flag,
            _ => {}
        }
    }
    settings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_field() {
        let settings = Settings {
            sound: false,
            mentions_only: true,
            preview: false,
            corner: Corner::TopLeft,
            flash: false,
            close_to_tray: false,
            do_not_disturb: true,
        };
        assert_eq!(parse(&serialize(&settings)), settings);
    }

    #[test]
    fn old_flash_key_is_ignored() {
        assert!(parse("flash=false").flash);
        assert!(!parse("taskbar_flash=false").flash);
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
        let changed = Settings {
            sound: false,
            ..Default::default()
        };
        save(&store, &changed);
        assert_eq!(load(&store), changed);
    }
}
