use calling::SPOKEN_LANGUAGES;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::{Dialog, DialogFooter};
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::{Icon, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::call_extras::confirm_record_action;
use super::call_transcript::TranscriptPanel;
use crate::app_state::AppState;
use crate::call::{CallModel, CaptionPosition, CaptionSize, record_actions};
use crate::theme;

const SETTINGS_WIDTH: f32 = 460.;
const CHIP_HEIGHT: f32 = 30.;
const SECTION_GAP: f32 = 14.;

pub fn record_submenu(
    popup: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
    app: &Entity<AppState>,
    model: &CallModel,
) -> PopupMenu {
    let actions = record_actions(model.recording, model.transcribing);
    let app = app.clone();
    popup.submenu_with_icon(
        Some(Icon::new(IconName::Video)),
        "Record and transcribe",
        window,
        cx,
        move |mut menu, _, _| {
            for (action, label) in actions.clone() {
                let app = app.clone();
                menu = menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                    confirm_record_action(&app, action, window, cx)
                }));
            }
            menu
        },
    )
}

pub fn transcript_and_settings_items(popup: PopupMenu, app: &Entity<AppState>) -> PopupMenu {
    let (transcript_app, settings_app) = (app.clone(), app.clone());
    popup
        .item(
            PopupMenuItem::new("Show transcript")
                .icon(IconName::LayoutList)
                .on_click(move |_, window, cx| open_transcript(&transcript_app, window, cx)),
        )
        .item(
            PopupMenuItem::new("Captions settings")
                .icon(IconName::Settings)
                .on_click(move |_, window, cx| open_captions_settings(&settings_app, window, cx)),
        )
}

fn open_transcript(app: &Entity<AppState>, window: &mut Window, cx: &mut App) {
    let panel_app = app.clone();
    let panel = cx.new(|cx| TranscriptPanel::new(panel_app, window, cx));
    app.update(cx, |state, cx| state.open_call_transcript(panel, cx));
}

fn choice_chip(
    id: String,
    label: &str,
    selected: bool,
    on_click: impl Fn(&mut App) + 'static,
) -> Stateful<Div> {
    let (background, foreground) = if selected {
        (theme::accent(), theme::white())
    } else {
        (theme::surface_raised(), theme::text())
    };
    div()
        .id(ElementId::Name(id.into()))
        .h(px(CHIP_HEIGHT))
        .px(px(12.))
        .flex()
        .items_center()
        .rounded_full()
        .bg(background)
        .text_color(foreground)
        .text_size(px(13.))
        .when(!selected, |chip| chip.hover(|chip| chip.bg(theme::row_hover())))
        .cursor_pointer()
        .child(label.to_owned())
        .on_click(move |_, _, cx| on_click(cx))
}

fn section(title: &'static str, chips: Vec<Stateful<Div>>) -> Div {
    v_flex()
        .gap(px(8.))
        .child(
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text_muted())
                .child(title),
        )
        .child(h_flex().flex_wrap().gap(px(8.)).children(chips))
}

pub fn open_captions_settings(app: &Entity<AppState>, window: &mut Window, cx: &mut App) {
    let app = app.clone();
    window.open_dialog(cx, move |dialog: Dialog, _, cx| {
        let state = app.read(cx);
        let language = state
            .call
            .as_ref()
            .and_then(|call| call.model.caption_language.clone());
        let (size, position) = (state.call_view.caption_size, state.call_view.caption_position);
        let languages = SPOKEN_LANGUAGES
            .iter()
            .map(|(tag, label)| {
                let app = app.clone();
                let chosen = *tag;
                choice_chip(
                    format!("captions-language-{tag}"),
                    label,
                    language.as_deref() == Some(tag),
                    move |cx| app.update(cx, |state, cx| state.choose_call_caption_language(chosen, cx)),
                )
            })
            .collect();
        let sizes = CaptionSize::ALL
            .iter()
            .map(|choice| {
                let app = app.clone();
                let choice = *choice;
                choice_chip(
                    format!("captions-size-{}", choice.meta()),
                    choice.label(),
                    size == choice,
                    move |cx| app.update(cx, |state, cx| state.set_call_caption_size(choice, cx)),
                )
            })
            .collect();
        let positions = CaptionPosition::ALL
            .iter()
            .map(|choice| {
                let app = app.clone();
                let choice = *choice;
                choice_chip(
                    format!("captions-position-{}", choice.meta()),
                    choice.label(),
                    position == choice,
                    move |cx| app.update(cx, |state, cx| state.set_call_caption_position(choice, cx)),
                )
            })
            .collect();
        dialog
            .title("Captions settings")
            .w(px(SETTINGS_WIDTH))
            .child(
                v_flex()
                    .id("captions-settings")
                    .gap(px(SECTION_GAP))
                    .child(section("Spoken language", languages))
                    .child(section("Size", sizes))
                    .child(section("Position", positions)),
            )
            .footer(
                DialogFooter::new().child(
                    Button::new("captions-settings-done")
                        .primary()
                        .label("Done")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
            )
    });
}
