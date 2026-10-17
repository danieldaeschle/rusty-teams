use gpui_kit::*;
use store::Store;

use crate::appearance::{self, AppIcon, AppearanceSettings, SystemTheme, ThemeChoice};
use crate::theme::{self, ColorScheme};
use crate::zoom;

#[derive(Default)]
pub struct ActiveAppearance {
    settings: AppearanceSettings,
    system: SystemTheme,
}

impl Global for ActiveAppearance {}

pub fn system_theme(appearance: WindowAppearance) -> SystemTheme {
    SystemTheme {
        dark: matches!(
            appearance,
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        ),
        high_contrast: appearance::system_high_contrast(),
    }
}

pub fn install(store: &Store, cx: &mut App) {
    let settings = appearance::load(store);
    let system = system_theme(cx.window_appearance());
    theme::set_active_scheme(settings.color_scheme.id);
    theme::set_active_shade(settings.theme.shade(system));
    zoom::set_factor(settings.zoom);
    cx.set_global(ActiveAppearance { settings, system });
}

pub fn current(cx: &App) -> AppearanceSettings {
    cx.try_global::<ActiveAppearance>()
        .map(|active| active.settings)
        .unwrap_or_default()
}

fn system(cx: &App) -> SystemTheme {
    cx.try_global::<ActiveAppearance>()
        .map(|active| active.system)
        .unwrap_or_default()
}

fn store_settings(settings: AppearanceSettings, store: &Store, cx: &mut App) {
    appearance::save(store, &settings);
    let system = system(cx);
    cx.set_global(ActiveAppearance { settings, system });
}

fn restyle(cx: &mut App) {
    let active = cx.global::<ActiveAppearance>();
    let shade = active.settings.theme.shade(active.system);
    let scheme = active.settings.color_scheme.id;
    if theme::scheme().id != scheme || theme::shade() != shade {
        theme::set_active_scheme(scheme);
        theme::set_active_shade(shade);
        theme::apply(cx);
    }
    cx.refresh_windows();
}

pub fn set_color_scheme(scheme: &'static ColorScheme, store: &Store, cx: &mut App) {
    let settings = AppearanceSettings {
        color_scheme: scheme,
        ..current(cx)
    };
    store_settings(settings, store, cx);
    restyle(cx);
}

pub fn set_theme(choice: ThemeChoice, store: &Store, cx: &mut App) {
    let settings = AppearanceSettings {
        theme: choice,
        ..current(cx)
    };
    store_settings(settings, store, cx);
    restyle(cx);
}

pub fn system_appearance_changed(appearance: WindowAppearance, cx: &mut App) {
    let system = system_theme(appearance);
    if cx.try_global::<ActiveAppearance>().is_none() || self::system(cx) == system {
        return;
    }
    let settings = current(cx);
    cx.set_global(ActiveAppearance { settings, system });
    restyle(cx);
}

pub fn set_app_icon(icon: AppIcon, store: &Store, cx: &mut App) {
    let settings = AppearanceSettings {
        app_icon: icon,
        ..current(cx)
    };
    store_settings(settings, store, cx);
    cx.refresh_windows();
}

pub fn set_zoom(zoom: f32, store: &Store, cx: &mut App) -> bool {
    let current = current(cx);
    if (current.zoom - zoom).abs() < f32::EPSILON {
        return false;
    }
    let settings = AppearanceSettings { zoom, ..current };
    store_settings(settings, store, cx);
    zoom::set_factor(zoom);
    cx.refresh_windows();
    true
}

#[cfg(test)]
mod tests {
    use gpui_kit::{TestAppContext, WindowAppearance};
    use store::Store;

    use super::{
        current, install, set_app_icon, set_color_scheme, set_theme, set_zoom,
        system_appearance_changed,
    };
    use crate::appearance::{self, AppIcon, AppearanceSettings, ThemeChoice};
    use crate::theme::{self, Shade};

    #[gpui_kit::test]
    fn switching_saves_and_applies_live(cx: &mut TestAppContext) {
        let store = Store::open_in_memory().expect("in-memory store");
        cx.update(|cx| {
            gpui_kit::init(cx);
            install(&store, cx);
            theme::set_thread_scheme(None);
            theme::set_thread_shade(None);
            assert_eq!(current(cx), AppearanceSettings::default());
            let teams = theme::scheme_by_id("teams").expect("teams scheme");
            set_color_scheme(teams, &store, cx);
            set_app_icon(AppIcon::MicrosoftTeams, &store, cx);
            assert_eq!(theme::scheme().id, "teams");
            assert_eq!(theme::accent(), theme::color(teams.dark.accent));
            assert_eq!(appearance::load(&store), current(cx));
            assert_eq!(current(cx).app_icon, AppIcon::MicrosoftTeams);
            set_theme(ThemeChoice::Light, &store, cx);
            assert_eq!(theme::shade(), Shade::Light);
            assert_eq!(theme::background(), theme::color(teams.light.background));
            assert_eq!(appearance::load(&store).theme, ThemeChoice::Light);
            set_color_scheme(theme::default_scheme(), &store, cx);
            set_theme(ThemeChoice::Dark, &store, cx);
            assert_eq!(theme::scheme().id, "rusty-teams");
            assert_eq!(theme::shade(), Shade::Dark);
        });
    }

    #[gpui_kit::test]
    fn follow_os_tracks_the_window_appearance_at_runtime(cx: &mut TestAppContext) {
        let store = Store::open_in_memory().expect("in-memory store");
        cx.update(|cx| {
            gpui_kit::init(cx);
            install(&store, cx);
            set_theme(ThemeChoice::FollowOperatingSystem, &store, cx);
            system_appearance_changed(WindowAppearance::Dark, cx);
            assert_eq!(theme::shade(), Shade::Dark);
            system_appearance_changed(WindowAppearance::Light, cx);
            assert_eq!(theme::shade(), Shade::Light);
            set_theme(ThemeChoice::Dark, &store, cx);
            system_appearance_changed(WindowAppearance::Light, cx);
            assert_eq!(theme::shade(), Shade::Dark);
        });
    }

    #[gpui_kit::test]
    fn zoom_is_saved_and_unchanged_steps_report_nothing(cx: &mut TestAppContext) {
        let store = Store::open_in_memory().expect("in-memory store");
        cx.update(|cx| {
            gpui_kit::init(cx);
            install(&store, cx);
            assert!(set_zoom(1.25, &store, cx));
            assert!(!set_zoom(1.25, &store, cx));
            assert_eq!(appearance::load(&store).zoom, 1.25);
            assert!(set_zoom(1., &store, cx));
        });
    }
}
