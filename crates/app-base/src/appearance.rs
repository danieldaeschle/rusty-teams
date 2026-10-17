use store::Store;

use crate::theme::{self, ColorScheme};

const META_KEY: &str = "appearance.settings";
const COLOR_SCHEME_KEY: &str = "color_scheme";
const APP_ICON_KEY: &str = "app_icon";
const RUSTY_TEAMS_ICON_PNG: &[u8] = include_bytes!("../../app/assets/icon/teams-fast-256.png");
const MICROSOFT_TEAMS_ICON_PNG: &[u8] =
    include_bytes!("../../app/assets/icon/microsoft-teams-256.png");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppIcon {
    #[default]
    RustyTeams,
    MicrosoftTeams,
}

impl AppIcon {
    pub const ALL: [AppIcon; 2] = [AppIcon::RustyTeams, AppIcon::MicrosoftTeams];

    pub fn key(self) -> &'static str {
        match self {
            AppIcon::RustyTeams => "rusty-teams",
            AppIcon::MicrosoftTeams => "microsoft-teams",
        }
    }

    pub fn from_key(key: &str) -> Option<AppIcon> {
        AppIcon::ALL.into_iter().find(|icon| icon.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            AppIcon::RustyTeams => "Rusty Teams",
            AppIcon::MicrosoftTeams => "Microsoft Teams",
        }
    }

    pub fn png(self) -> &'static [u8] {
        match self {
            AppIcon::RustyTeams => RUSTY_TEAMS_ICON_PNG,
            AppIcon::MicrosoftTeams => MICROSOFT_TEAMS_ICON_PNG,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppearanceSettings {
    pub color_scheme: &'static ColorScheme,
    pub app_icon: AppIcon,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        AppearanceSettings {
            color_scheme: theme::default_scheme(),
            app_icon: AppIcon::default(),
        }
    }
}

pub fn load(store: &Store) -> AppearanceSettings {
    store
        .meta(META_KEY)
        .ok()
        .flatten()
        .map(|text| parse(&text))
        .unwrap_or_default()
}

pub fn save(store: &Store, settings: &AppearanceSettings) {
    let _ = store.set_meta(META_KEY, &serialize(settings));
}

fn serialize(settings: &AppearanceSettings) -> String {
    format!(
        "{COLOR_SCHEME_KEY}={}\n{APP_ICON_KEY}={}",
        settings.color_scheme.id,
        settings.app_icon.key()
    )
}

fn parse(text: &str) -> AppearanceSettings {
    let mut settings = AppearanceSettings::default();
    for line in text.lines() {
        match line.split_once('=') {
            Some((COLOR_SCHEME_KEY, value)) => {
                if let Some(scheme) = theme::scheme_by_id(value) {
                    settings.color_scheme = scheme;
                }
            }
            Some((APP_ICON_KEY, value)) => {
                if let Some(icon) = AppIcon::from_key(value) {
                    settings.app_icon = icon;
                }
            }
            _ => {}
        }
    }
    settings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_rusty_teams_look() {
        let settings = AppearanceSettings::default();
        assert_eq!(settings.color_scheme.id, "rusty-teams");
        assert_eq!(settings.app_icon, AppIcon::RustyTeams);
    }

    #[test]
    fn round_trips_every_field() {
        let settings = AppearanceSettings {
            color_scheme: theme::SCHEMES[1],
            app_icon: AppIcon::MicrosoftTeams,
        };
        assert_eq!(parse(&serialize(&settings)), settings);
    }

    #[test]
    fn unknown_values_fall_back_to_defaults() {
        assert_eq!(parse(""), AppearanceSettings::default());
        assert_eq!(
            parse("color_scheme=solarized\napp_icon=clippy\njunk"),
            AppearanceSettings::default()
        );
    }

    #[test]
    fn persists_in_store() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(load(&store), AppearanceSettings::default());
        let changed = AppearanceSettings {
            color_scheme: theme::scheme_by_id("teams").unwrap(),
            app_icon: AppIcon::MicrosoftTeams,
        };
        save(&store, &changed);
        assert_eq!(load(&store), changed);
    }

    #[test]
    fn both_icons_are_png() {
        for icon in AppIcon::ALL {
            assert!(icon.png().starts_with(b"\x89PNG"), "{}", icon.key());
        }
    }
}
