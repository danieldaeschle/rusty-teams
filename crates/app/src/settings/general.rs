use gpui_kit::assets::IconName;
use gpui_kit::component::{
    button::Button,
    h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
    v_flex,
};
use gpui_kit::*;
use teams_core::{ChatSettingChange, TranslationBehavior};

use super::account::{UP_ARROW, account_select};
use super::rows::{Choice, caption, control_row, select, toggle_row};
use crate::app_state::AppState;
use crate::notify::NotificationCenter;
use crate::theme;
use crate::views::widgets::icon;

const BEHAVIORS: [(&str, TranslationBehavior); 3] = [
    ("Ask me before translating", TranslationBehavior::Ask),
    ("Auto-translate all messages", TranslationBehavior::Auto),
    ("Never translate", TranslationBehavior::Never),
];
const CHIP_HEIGHT: f32 = 24.;
const LANGUAGE_MENU_HEIGHT: f32 = 320.;

pub fn render(center: &Entity<NotificationCenter>, app: &Entity<AppState>, cx: &App) -> Div {
    let close_to_tray = center.read(cx).settings().close_to_tray;
    let toggle_center = center.clone();
    v_flex()
        .w_full()
        .child(caption("SYSTEM"))
        .child(
            toggle_row(
                "settings-close-to-tray",
                "On close, keep the application running",
                "The title bar X hides the window to the tray",
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
        .child(caption("COMPOSE BOX"))
        .child(account_select(
            app,
            (
                "settings-up-arrow",
                "Up arrow key behaviour in compose box",
                "Choose what happens when you press Up arrow at the beginning of the compose box.",
            ),
            &UP_ARROW,
            app.read(cx).account_chat_settings().up_arrow,
            ChatSettingChange::UpArrow,
        ))
        .child(caption("TRANSLATION"))
        .children(translation_rows(app, cx))
}

fn translation_rows(app: &Entity<AppState>, cx: &App) -> Vec<AnyElement> {
    let state = app.read(cx);
    if state.translation.settings.is_none() {
        return vec![
            control_row("Translation", "Translation settings not loaded yet", div())
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
    let behaviors = BEHAVIORS
        .into_iter()
        .map(|(label, choice)| {
            let app = app.clone();
            Choice::new(label, behavior == choice, move |cx| {
                app.update(cx, |state, cx| state.set_translation_behavior(choice, cx))
            })
        })
        .collect();
    let behavior_label = BEHAVIORS
        .iter()
        .find(|(_, choice)| *choice == behavior)
        .map_or("", |(label, _)| label);
    vec![
        control_row(
            "Translate messages into this language",
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
            "How to handle messages in other languages",
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
            "Never translate messages in these languages",
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
        .label("+ Add language")
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
