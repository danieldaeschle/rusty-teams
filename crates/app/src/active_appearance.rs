use gpui_kit::*;
use store::Store;

use crate::appearance::{self, AppIcon, AppearanceSettings};
use crate::theme::{self, ColorScheme};

#[derive(Default)]
pub struct ActiveAppearance {
    settings: AppearanceSettings,
}

impl Global for ActiveAppearance {}

pub fn install(store: &Store, cx: &mut App) {
    let settings = appearance::load(store);
    theme::set_active_scheme(settings.color_scheme.id);
    cx.set_global(ActiveAppearance { settings });
}

pub fn current(cx: &App) -> AppearanceSettings {
    cx.try_global::<ActiveAppearance>()
        .map(|active| active.settings)
        .unwrap_or_default()
}

pub fn set_color_scheme(scheme: &'static ColorScheme, store: &Store, cx: &mut App) {
    let settings = AppearanceSettings {
        color_scheme: scheme,
        ..current(cx)
    };
    appearance::save(store, &settings);
    theme::set_active_scheme(scheme.id);
    theme::apply(cx);
    cx.set_global(ActiveAppearance { settings });
    cx.refresh_windows();
}

pub fn set_app_icon(icon: AppIcon, store: &Store, cx: &mut App) {
    let settings = AppearanceSettings {
        app_icon: icon,
        ..current(cx)
    };
    appearance::save(store, &settings);
    cx.set_global(ActiveAppearance { settings });
    cx.refresh_windows();
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;
    use store::Store;

    use super::{current, install, set_app_icon, set_color_scheme};
    use crate::appearance::{self, AppIcon, AppearanceSettings};
    use crate::theme;

    #[gpui_kit::test]
    fn switching_saves_and_applies_live(cx: &mut TestAppContext) {
        let store = Store::open_in_memory().expect("in-memory store");
        cx.update(|cx| {
            gpui_kit::init(cx);
            install(&store, cx);
            theme::set_thread_scheme(None);
            assert_eq!(current(cx), AppearanceSettings::default());
            let teams = theme::scheme_by_id("teams").expect("teams scheme");
            set_color_scheme(teams, &store, cx);
            set_app_icon(AppIcon::MicrosoftTeams, &store, cx);
            assert_eq!(theme::scheme().id, "teams");
            assert_eq!(theme::accent(), theme::color(teams.accent));
            assert_eq!(appearance::load(&store), current(cx));
            assert_eq!(current(cx).app_icon, AppIcon::MicrosoftTeams);
            set_color_scheme(theme::default_scheme(), &store, cx);
            assert_eq!(theme::scheme().id, "rusty-teams");
        });
    }
}
