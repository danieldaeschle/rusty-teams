use calling::{MicSettings, NoiseSuppression};
use store::Store;

const META_KEY: &str = "call_mic";

pub fn load_mic(store: &Store) -> MicSettings {
    store
        .meta(META_KEY)
        .ok()
        .flatten()
        .map(|text| parse(&text))
        .unwrap_or_default()
}

pub fn save_mic(store: &Store, settings: MicSettings) {
    let _ = store.set_meta(META_KEY, &serialize(settings));
}

fn serialize(settings: MicSettings) -> String {
    format!(
        "noise={}\nmusic={}",
        settings.noise_suppression.key(),
        settings.music_mode
    )
}

fn parse(text: &str) -> MicSettings {
    let mut settings = MicSettings::default();
    for (key, value) in text.lines().filter_map(|line| line.split_once('=')) {
        match key {
            "noise" => {
                settings.noise_suppression = NoiseSuppression::from_key(value).unwrap_or_default()
            }
            "music" => settings.music_mode = value == "true",
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
        let settings = MicSettings {
            noise_suppression: NoiseSuppression::Off,
            music_mode: true,
        };
        assert_eq!(parse(&serialize(settings)), settings);
    }

    #[test]
    fn every_noise_level_round_trips() {
        for noise_suppression in NoiseSuppression::ALL {
            let settings = MicSettings {
                noise_suppression,
                music_mode: false,
            };
            assert_eq!(parse(&serialize(settings)), settings);
        }
    }

    #[test]
    fn garbage_gives_defaults() {
        assert_eq!(parse(""), MicSettings::default());
        assert_eq!(parse("noise=loud\nmusic"), MicSettings::default());
    }

    #[test]
    fn persists_in_store() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(load_mic(&store), MicSettings::default());
        let changed = MicSettings {
            noise_suppression: NoiseSuppression::Off,
            music_mode: false,
        };
        save_mic(&store, changed);
        assert_eq!(load_mic(&store), changed);
    }
}
