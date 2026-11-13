use gpui_kit::assets::IconName;
use gpui_kit::component::{
    button::Button,
    h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::{ChatSettingChange, TranslationBehavior};

use super::account::{UP_ARROW, account_select};
use super::rows::{Choice, caption, control_row, select, toggle_row};
use crate::app_state::AppState;
use crate::i18n::tr;
use crate::notify::NotificationCenter;
use crate::theme;
use crate::views::widgets::icon;

const CHIP_HEIGHT: f32 = 24.;
const DISABLED_OPACITY: f32 = 0.4;
const LANGUAGE_MENU_HEIGHT: f32 = 320.;

fn startup_rows(center: &Entity<NotificationCenter>, cx: &App) -> [Stateful<Div>; 2] {
    let startup = center.read(cx).startup();
    let (auto_start_center, background_center) = (center.clone(), center.clone());
    [
        toggle_row(
            "settings-auto-start",
            tr("settings.general.auto_start"),
            "",
            theme::text_muted(),
            startup.auto_start,
        )
        .on_click(move |_, _, cx| {
            auto_start_center.update(cx, |center, cx| {
                center.set_auto_start(!startup.auto_start, cx)
            })
        }),
        toggle_row(
            "settings-open-in-background",
            tr("settings.general.open_in_background"),
            "",
            theme::text_muted(),
            startup.open_in_background,
        )
        .when(!startup.auto_start, |row| row.opacity(DISABLED_OPACITY))
        .when(startup.auto_start, |row| {
            row.on_click(move |_, _, cx| {
                background_center.update(cx, |center, cx| {
                    center.set_open_in_background(!startup.open_in_background, cx)
                })
            })
        }),
    ]
}

pub fn render(center: &Entity<NotificationCenter>, app: &Entity<AppState>, cx: &App) -> Div {
    let close_to_tray = center.read(cx).settings().close_to_tray;
    let toggle_center = center.clone();
    v_flex()
        .w_full()
        .child(caption(tr("settings.caption.system")))
        .children(startup_rows(center, cx))
        .child(
            toggle_row(
                "settings-close-to-tray",
                tr("settings.general.close_to_tray"),
                tr("settings.general.close_to_tray_detail"),
                theme::text_muted(),
                close_to_tray,
            )
            .on_click(move |_, _, cx| {
                toggle_center.update(cx, |center, cx| {
                    center.update_settings(cx, |settings| {
                        settings.close_to_tray = !settings.close_to_tray
                    })
                })
            }),
        )
        .children(external_browser_rows(app, cx))
        .child(caption(tr("settings.caption.compose_box")))
        .child(account_select(
            app,
            (
                "settings-up-arrow",
                tr("settings.general.up_arrow"),
                tr("settings.general.up_arrow_detail"),
            ),
            &UP_ARROW,
            app.read(cx).account_chat_settings().up_arrow,
            ChatSettingChange::UpArrow,
        ))
        .child(caption(tr("settings.caption.translation")))
        .children(translation_rows(app, cx))
}

fn external_browser_rows(app: &Entity<AppState>, cx: &App) -> Vec<AnyElement> {
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (app, cx);
        return Vec::new();
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use crate::external_browser::{self, ExternalBrowser};

        let selected = external_browser::load(&app.read(cx).store);
        let current_label = external_browser::available_choices()
            .iter()
            .find(|choice| choice.id == selected.id())
            .map(|choice| {
                if external_browser::is_system_id(&choice.id) {
                    tr("settings.general.external_browser_system")
                } else {
                    choice.label.clone()
                }
            })
            .unwrap_or_else(|| tr("settings.general.external_browser_system"));
        let choices: Vec<Choice> = external_browser::available_choices()
            .iter()
            .map(|choice| {
                let label = if external_browser::is_system_id(&choice.id) {
                    tr("settings.general.external_browser_system")
                } else {
                    choice.label.clone()
                };
                let is_selected = selected.id() == choice.id;
                let (app, id) = (app.clone(), choice.id.clone());
                Choice::new(label, is_selected, move |cx| {
                    app.update(cx, |state, cx| {
                        let browser = if external_browser::is_system_id(&id) {
                            ExternalBrowser::System
                        } else {
                            ExternalBrowser::Program(id.clone())
                        };
                        external_browser::save(&state.store, &browser);
                        cx.notify();
                    });
                })
            })
            .collect();
        vec![
            caption(tr("settings.caption.browser")).into_any_element(),
            control_row(
                tr("settings.general.external_browser"),
                tr("settings.general.external_browser_detail"),
                select("settings-external-browser", current_label, false, choices),
            )
            .into_any_element(),
        ]
    }
}

fn translation_rows(app: &Entity<AppState>, cx: &App) -> Vec<AnyElement> {
    let state = app.read(cx);
    if state.translation.settings.is_none() {
        return vec![
            control_row(
                tr("settings.general.translation_title"),
                tr("settings.general.translation_not_loaded"),
                div(),
            )
            .into_any_element(),
        ];
    }
    let translation = &state.translation;
    let target = translation.target();
    let behavior = translation.behavior();
    let targets = translation
        .languages
        .iter()
        .map(|language| {
            let (app, code) = (app.clone(), language.code.clone());
            Choice::new(language.name.clone(), language.code == target, move |cx| {
                app.update(cx, |state, cx| state.set_translation_target(&code, cx))
            })
        })
        .collect();
    let behavior_options = [
        (
            tr("settings.general.behavior_ask"),
            TranslationBehavior::Ask,
        ),
        (
            tr("settings.general.behavior_auto"),
            TranslationBehavior::Auto,
        ),
        (
            tr("settings.general.behavior_never"),
            TranslationBehavior::Never,
        ),
    ];
    let behavior_label = behavior_options
        .iter()
        .find(|(_, choice)| *choice == behavior)
        .map_or(String::new(), |(label, _)| label.clone());
    let behaviors = behavior_options
        .into_iter()
        .map(|(label, choice)| {
            let app = app.clone();
            Choice::new(label, behavior == choice, move |cx| {
                app.update(cx, |state, cx| state.set_translation_behavior(choice, cx))
            })
        })
        .collect();
    vec![
        control_row(
            tr("settings.general.translate_into"),
            "Read from your Teams account",
            select(
                "settings-translation-target",
                translation.language_name(&target),
                false,
                targets,
            ),
        )
        .into_any_element(),
        control_row(
            tr("settings.general.behavior_title"),
            "",
            select(
                "settings-translation-behavior",
                behavior_label,
                false,
                behaviors,
            ),
        )
        .into_any_element(),
        control_row(
            tr("settings.general.never_translate"),
            "",
            never_chips(app, &target, cx),
        )
        .into_any_element(),
    ]
}

fn never_chips(app: &Entity<AppState>, target: &str, cx: &App) -> Div {
    let translation = &app.read(cx).translation;
    let never: Vec<String> = translation
        .display_codes()
        .into_iter()
        .filter(|code| code != target)
        .collect();
    let chips = never.iter().map(|code| {
        let (remove_app, remove_code) = (app.clone(), code.clone());
        h_flex()
            .h(px(CHIP_HEIGHT))
            .pl(px(8.))
            .gap(px(4.))
            .items_center()
            .rounded(px(6.))
            .bg(theme::surface_raised())
            .text_size(px(12.))
            .text_color(theme::text_strong())
            .child(translation.language_name(code))
            .child(
                div()
                    .id(SharedString::from(format!("settings-never-remove-{code}")))
                    .w(px(16.))
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .child(icon(IconName::X, 10., theme::text_faint()))
                    .on_click(move |_, _, cx| {
                        remove_app.update(cx, |state, cx| {
                            state.allow_translating_language(&remove_code, cx)
                        })
                    }),
            )
    });
    let addable = translation
        .languages
        .iter()
        .filter(|language| language.code != target && !never.contains(&language.code))
        .map(|language| {
            let (app, code) = (app.clone(), language.code.clone());
            Choice::new(language.name.clone(), false, move |cx| {
                app.update(cx, |state, cx| state.never_translate_language(&code, cx))
            })
        })
        .collect();
    h_flex()
        .flex_wrap()
        .justify_end()
        .gap(px(6.))
        .items_center()
        .children(chips)
        .child(add_language_button(addable))
}

fn add_language_button(choices: Vec<Choice>) -> impl IntoElement {
    Button::new("settings-never-add")
        .outline()
        .h(px(CHIP_HEIGHT))
        .px(px(8.))
        .rounded(px(6.))
        .text_size(px(12.))
        .text_color(theme::accent_text())
        .label(format!("+ {}", tr("common.add")))
        .dropdown_menu(move |menu, _, _| {
            choices.iter().fold(
                menu.max_h(px(LANGUAGE_MENU_HEIGHT)).scrollable(true),
                |menu, choice| {
                    let choose = choice.choose.clone();
                    menu.item(
                        PopupMenuItem::new(choice.label.clone())
                            .on_click(move |_, _, cx| choose(cx)),
                    )
                },
            )
        })
}
