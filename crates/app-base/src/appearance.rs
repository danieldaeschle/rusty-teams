use store::Store;

use crate::theme::{self, ColorScheme, Shade};

const META_KEY: &str = "appearance.settings";
const COLOR_SCHEME_KEY: &str = "color_scheme";
const APP_ICON_KEY: &str = "app_icon";
const THEME_KEY: &str = "theme";
const ZOOM_KEY: &str = "zoom";
pub const ZOOM_STEPS: [f32; 14] = [
    0.5, 0.67, 0.75, 0.8, 0.9, 1., 1.1, 1.25, 1.5, 1.75, 2., 2.5, 3., 4.,
];
pub const DEFAULT_ZOOM: f32 = 1.;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeChoice {
    FollowOperatingSystem,
    Light,
    #[default]
    Dark,
    HighContrast,
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 4] = [
        ThemeChoice::FollowOperatingSystem,
        ThemeChoice::Light,
        ThemeChoice::Dark,
        ThemeChoice::HighContrast,
    ];

    pub fn key(self) -> &'static str {
        match self {
            ThemeChoice::FollowOperatingSystem => "followOsTheme",
            ThemeChoice::Light => "light",
            ThemeChoice::Dark => "dark",
            ThemeChoice::HighContrast => "contrast",
        }
    }

    pub fn from_key(key: &str) -> Option<ThemeChoice> {
        ThemeChoice::ALL
            .into_iter()
            .find(|choice| choice.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            ThemeChoice::FollowOperatingSystem => "Follow operating system theme",
            ThemeChoice::Light => "Light",
            ThemeChoice::Dark => "Dark",
            ThemeChoice::HighContrast => "High contrast",
        }
    }

    pub fn shade(self, system: SystemTheme) -> Shade {
        match self {
            ThemeChoice::Light => Shade::Light,
            ThemeChoice::Dark => Shade::Dark,
            ThemeChoice::HighContrast => Shade::HighContrast,
            ThemeChoice::FollowOperatingSystem if system.high_contrast => Shade::HighContrast,
            ThemeChoice::FollowOperatingSystem if system.dark => Shade::Dark,
            ThemeChoice::FollowOperatingSystem => Shade::Light,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SystemTheme {
    pub dark: bool,
    pub high_contrast: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AppearanceSettings {
    pub color_scheme: &'static ColorScheme,
    pub app_icon: AppIcon,
    pub theme: ThemeChoice,
    pub zoom: f32,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        AppearanceSettings {
            color_scheme: theme::default_scheme(),
            app_icon: AppIcon::default(),
            theme: ThemeChoice::default(),
            zoom: DEFAULT_ZOOM,
        }
    }
}

pub fn nearest_zoom_step(zoom: f32) -> f32 {
    ZOOM_STEPS
        .into_iter()
        .min_by(|first, second| (first - zoom).abs().total_cmp(&(second - zoom).abs()))
        .unwrap_or(DEFAULT_ZOOM)
}

pub fn zoom_in(zoom: f32) -> f32 {
    ZOOM_STEPS
        .into_iter()
        .find(|step| *step > zoom + f32::EPSILON)
        .unwrap_or(ZOOM_STEPS[ZOOM_STEPS.len() - 1])
}

pub fn zoom_out(zoom: f32) -> f32 {
    ZOOM_STEPS
        .into_iter()
        .rev()
        .find(|step| *step < zoom - f32::EPSILON)
        .unwrap_or(ZOOM_STEPS[0])
}

pub fn zoom_percent(zoom: f32) -> u32 {
    (zoom * 100. - 0.001).ceil().max(0.) as u32
}

#[cfg(windows)]
pub fn system_high_contrast() -> bool {
    use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    };
    let mut info = HIGHCONTRASTW {
        cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            info.cbSize,
            Some(std::ptr::from_mut(&mut info).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    read.is_ok() && info.dwFlags.0 & HCF_HIGHCONTRASTON.0 != 0
}

#[cfg(not(windows))]
pub fn system_high_contrast() -> bool {
    false
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
        "{COLOR_SCHEME_KEY}={}\n{APP_ICON_KEY}={}\n{THEME_KEY}={}\n{ZOOM_KEY}={}",
        settings.color_scheme.id,
        settings.app_icon.key(),
        settings.theme.key(),
        settings.zoom
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
            Some((THEME_KEY, value)) => {
                if let Some(choice) = ThemeChoice::from_key(value) {
                    settings.theme = choice;
                }
            }
            Some((ZOOM_KEY, value)) => {
                if let Ok(zoom) = value.parse::<f32>() {
                    settings.zoom = nearest_zoom_step(zoom);
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
        assert_eq!(settings.theme, ThemeChoice::Dark);
        assert_eq!(settings.zoom, 1.);
    }

    #[test]
    fn round_trips_every_field() {
        let settings = AppearanceSettings {
            color_scheme: theme::SCHEMES[1],
            app_icon: AppIcon::MicrosoftTeams,
            theme: ThemeChoice::FollowOperatingSystem,
            zoom: 1.25,
        };
        assert_eq!(parse(&serialize(&settings)), settings);
    }

    #[test]
    fn unknown_values_fall_back_to_defaults() {
        assert_eq!(parse(""), AppearanceSettings::default());
        assert_eq!(
            parse("color_scheme=solarized\napp_icon=clippy\ntheme=sepia\nzoom=big\njunk"),
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
            theme: ThemeChoice::Light,
            zoom: 0.9,
        };
        save(&store, &changed);
        assert_eq!(load(&store), changed);
    }

    #[test]
    fn older_settings_without_theme_and_zoom_keep_the_dark_default() {
        let settings = parse("color_scheme=teams\napp_icon=rusty-teams");
        assert_eq!(settings.theme, ThemeChoice::Dark);
        assert_eq!(settings.zoom, DEFAULT_ZOOM);
        assert_eq!(parse("zoom=1.3").zoom, 1.25);
    }

    #[test]
    fn follow_os_takes_dark_light_and_forced_contrast_from_the_system() {
        let follow = ThemeChoice::FollowOperatingSystem;
        let dark = SystemTheme {
            dark: true,
            high_contrast: false,
        };
        assert_eq!(follow.shade(dark), Shade::Dark);
        assert_eq!(follow.shade(SystemTheme::default()), Shade::Light);
        let forced = SystemTheme {
            dark: false,
            high_contrast: true,
        };
        assert_eq!(follow.shade(forced), Shade::HighContrast);
        assert_eq!(ThemeChoice::Light.shade(dark), Shade::Light);
        assert_eq!(ThemeChoice::HighContrast.shade(dark), Shade::HighContrast);
    }

    #[test]
    fn theme_keys_are_the_teams_option_keys() {
        let keys: Vec<_> = ThemeChoice::ALL.iter().map(|choice| choice.key()).collect();
        assert_eq!(keys, ["followOsTheme", "light", "dark", "contrast"]);
    }

    #[test]
    fn zoom_walks_the_steps_and_stops_at_both_ends() {
        assert_eq!(zoom_in(1.), 1.1);
        assert_eq!(zoom_out(1.), 0.9);
        assert_eq!(zoom_in(4.), 4.);
        assert_eq!(zoom_out(0.5), 0.5);
        assert_eq!(zoom_out(zoom_in(1.25)), 1.25);
        assert_eq!(zoom_percent(0.67), 67);
        assert_eq!(zoom_percent(1.1), 110);
        assert_eq!(zoom_percent(1.), 100);
    }

    #[test]
    fn both_icons_are_png() {
        for icon in AppIcon::ALL {
            assert!(icon.png().starts_with(b"\x89PNG"), "{}", icon.key());
        }
    }
}
