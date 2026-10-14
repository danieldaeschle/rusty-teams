use std::path::PathBuf;

use calling::{CameraEffects, HoldState};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use gpui_kit::*;

use super::call_organizer::pill;
use super::call_view::{BANNER_HEIGHT, MENU_WIDTH};
use super::transfer_picker::{PickerMode, TransferPicker};
use super::widgets::icon;
use crate::app_state::AppState;
use crate::call::{BackgroundPick, CallModel, RecordAction, RecordingChip};
use crate::theme;

const STAGE_RADIUS: f32 = 12.;
const THUMBNAIL_WIDTH: f32 = 64.;
const THUMBNAIL_HEIGHT: f32 = 36.;
const THUMBNAIL_GAP: f32 = 6.;
const MENU_PADDING: f32 = 24.;
const BACKDROP_OPACITY: f32 = 0.55;
const CARD_WIDTH: f32 = 420.;

fn dark_text() -> Hsla {
    hsla(0., 0., 0.08, 1.)
}

pub fn recording_chip(model: &CallModel) -> Option<AnyElement> {
    let chip = crate::call::recording_chip(model.recording, model.recording_audio_only, model.transcribing)?;
    let color = match chip {
        RecordingChip::Transcribing => theme::accent_text(),
        RecordingChip::Recording | RecordingChip::RecordingAudio => theme::red(),
    };
    Some(
        h_flex()
            .id("call-recording")
            .flex_none()
            .gap(px(6.))
            .items_center()
            .text_size(px(13.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(color)
            .child(div().size(px(9.)).rounded_full().bg(color))
            .child(chip.label())
            .into_any_element(),
    )
}

pub fn return_button(app: &Entity<AppState>, model: &CallModel) -> Option<AnyElement> {
    model.breakout.as_ref().filter(|room| room.main.is_some())?;
    let app = app.clone();
    Some(
        Button::new("call-return-main")
            .label("Return to main meeting")
            .on_click(move |_, _, cx| app.update(cx, |state, cx| state.return_to_main_meeting(cx)))
            .into_any_element(),
    )
}

pub fn room_chip(model: &CallModel) -> Option<AnyElement> {
    model.breakout.as_ref()?;
    Some(
        div()
            .flex_none()
            .text_size(px(13.))
            .text_color(theme::text_muted())
            .child("Breakout room")
            .into_any_element(),
    )
}

pub fn hold_panel(app: &Entity<AppState>, model: &CallModel) -> Option<AnyElement> {
    let (title, detail) = match model.hold {
        HoldState::Active => return None,
        HoldState::Local => (
            "On hold".to_owned(),
            format!(
                "{} cannot hear you and you cannot hear them.",
                model.peer_name
            ),
        ),
        HoldState::Remote => (
            "You're on hold".to_owned(),
            format!("{} put the call on hold.", model.peer_name),
        ),
    };
    let resume = (model.hold == HoldState::Local).then(|| {
        let app = app.clone();
        Button::new("call-resume")
            .label("Resume")
            .on_click(move |_, _, cx| app.update(cx, |state, cx| state.toggle_call_hold(cx)))
    });
    Some(
        v_flex()
            .id("call-hold")
            .flex_1()
            .min_h_0()
            .items_center()
            .justify_center()
            .gap(px(12.))
            .child(
                div()
                    .size(px(96.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(theme::surface_raised())
                    .child(icon(IconName::Clock, 48., theme::text())),
            )
            .child(
                div()
                    .text_size(px(22.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .text_color(theme::text_muted())
                    .child(detail),
            )
            .children(resume)
            .into_any_element(),
    )
}

pub fn whiteboard_stage(app: &Entity<AppState>, model: &CallModel) -> Option<AnyElement> {
    let label = model.whiteboard_label()?;
    let open = model.whiteboard_url().is_some().then(|| {
        let app = app.clone();
        Button::new("call-whiteboard-open")
            .label("Open in browser")
            .icon(IconName::ExternalLink)
            .on_click(move |_, _, cx| app.update(cx, |state, cx| state.open_call_whiteboard(cx)))
    });
    Some(
        v_flex()
            .id("call-whiteboard-stage")
            .flex_1()
            .min_w_0()
            .h_full()
            .items_center()
            .justify_center()
            .gap(px(12.))
            .rounded(px(STAGE_RADIUS))
            .bg(hsla(0., 0., 0.04, 1.))
            .text_color(theme::white())
            .child(icon(IconName::Pencil, 56., theme::white()))
            .child(
                div()
                    .text_size(px(18.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(label),
            )
            .children(open)
            .into_any_element(),
    )
}

pub fn consult_banner(app: &Entity<AppState>, state: &AppState) -> Option<AnyElement> {
    let call = state.call.as_ref()?;
    let consult = call.consult.as_ref()?;
    state.held_call.as_ref()?;
    let ready = consult.ready_to_transfer(&call.model);
    let first_name = consult
        .held_name
        .split_whitespace()
        .next()
        .unwrap_or("call")
        .to_owned();
    let (transfer_app, resume_app) = (app.clone(), app.clone());
    let transfer = pill("call-consult-transfer", "Transfer", true)
        .debug_selector(|| "shell/islands/main/call-header/call-consult/transfer".into());
    let transfer = if ready {
        transfer.on_click(move |_, _, cx| {
            transfer_app.update(cx, |state, cx| state.transfer_consulted(cx))
        })
    } else {
        transfer.opacity(0.5).cursor_default()
    };
    let resume = pill("call-consult-resume", format!("Resume {first_name}"), false)
        .debug_selector(|| "shell/islands/main/call-header/call-consult/resume".into())
        .on_click(move |_, _, cx| resume_app.update(cx, |state, cx| state.resume_held_call(cx)));
    Some(
        h_flex()
            .id("call-consult-banner")
            .debug_selector(|| "shell/islands/main/call-header/call-consult".into())
            .w_full()
            .h(px(BANNER_HEIGHT))
            .flex_none()
            .px(px(20.))
            .gap(px(12.))
            .items_center()
            .justify_center()
            .bg(theme::amber())
            .text_color(dark_text())
            .text_size(px(13.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(div().min_w_0().truncate().child(consult.banner_text()))
            .child(transfer)
            .child(resume)
            .into_any_element(),
    )
}

pub fn consent_overlay(app: &Entity<AppState>, model: &CallModel) -> Option<AnyElement> {
    if !model.consent_required {
        return None;
    }
    let app = app.clone();
    Some(
        div()
            .id("call-consent-layer")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("call-consent-backdrop")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(black().opacity(BACKDROP_OPACITY))
                    .occlude(),
            )
            .child(
                v_flex()
                    .id("call-consent-card")
                    .w(px(CARD_WIDTH))
                    .max_w(relative(1.))
                    .p(px(20.))
                    .gap(px(12.))
                    .rounded(px(10.))
                    .bg(theme::popup())
                    .text_color(theme::text())
                    .occlude()
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .items_center()
                            .child(div().size(px(10.)).rounded_full().bg(theme::red()))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("This meeting is being recorded"),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(theme::text_muted())
                            .child("Your microphone and camera stay off until you accept."),
                    )
                    .child(
                        h_flex().w_full().justify_end().child(
                            Button::new("call-consent-ok")
                                .label("OK")
                                .on_click(move |_, _, cx| {
                                    app.update(cx, |state, cx| state.accept_recording_notice(cx))
                                }),
                        ),
                    ),
            )
            .into_any_element(),
    )
}

pub fn confirm_record_action(app: &Entity<AppState>, action: RecordAction, window: &mut Window, cx: &mut App) {
    let Some(title) = action.confirm_title() else {
        app.update(cx, |state, cx| state.run_call_record_action(action, cx));
        return;
    };
    let app = app.clone();
    window.open_alert_dialog(cx, move |alert, _, _| {
        let app = app.clone();
        let cancel = Button::new("recording-cancel").label("Cancel").on_click(|_, window, cx| window.close_dialog(cx));
        let stop = Button::new("recording-stop").label("Stop").danger().on_click(move |_, window, cx| {
            app.update(cx, |state, cx| state.run_call_record_action(action, cx));
            window.close_dialog(cx);
        });
        alert
            .title(title)
            .description("The file is saved to the organizer's OneDrive and the link arrives in the meeting chat.")
            .footer(DialogFooter::new().child(cancel).child(stop))
            .on_ok(|_, _, _| false)
    });
}

pub fn open_transfer_picker(app: &Entity<AppState>, window: &mut Window, cx: &mut App) {
    let candidates = app.read(cx).transfer_candidates();
    let picker_app = app.clone();
    let picker = cx.new(|cx| TransferPicker::new(picker_app, candidates, PickerMode::Transfer, window, cx));
    app.update(cx, |state, cx| state.open_transfer_picker(picker, cx));
}

pub fn open_invite_picker(app: &Entity<AppState>, window: &mut Window, cx: &mut App) {
    let candidates = app.read(cx).invite_candidates();
    let picker_app = app.clone();
    let picker = cx.new(|cx| TransferPicker::new(picker_app, candidates, PickerMode::Invite, window, cx));
    app.update(cx, |state, cx| state.open_invite_picker(picker, cx));
}

#[derive(Clone)]
pub struct CameraMenu {
    pub pick: BackgroundPick,
    pub images: Vec<(String, String, Option<PathBuf>)>,
    pub customs: Vec<PathBuf>,
    pub effects: CameraEffects,
}

impl CameraMenu {
    pub fn of(state: &AppState) -> Self {
        let library = &state.call_backgrounds;
        CameraMenu {
            pick: state.call_background.clone(),
            images: library
                .shown()
                .iter()
                .map(|image| {
                    (
                        image.id.clone(),
                        image.name.clone(),
                        library.thumbnail(image),
                    )
                })
                .collect(),
            customs: library.customs.clone(),
            effects: state.call_effects,
        }
    }
}

fn thumbnail(
    app: &Entity<AppState>,
    index: usize,
    label: String,
    path: Option<PathBuf>,
    selected: bool,
    pick: BackgroundPick,
) -> AnyElement {
    let app = app.clone();
    let tooltip = label.clone();
    div()
        .id(("call-background", index))
        .w(px(THUMBNAIL_WIDTH))
        .h(px(THUMBNAIL_HEIGHT))
        .flex_none()
        .overflow_hidden()
        .rounded(px(6.))
        .border_2()
        .border_color(if selected {
            theme::accent()
        } else {
            transparent_black()
        })
        .bg(theme::surface())
        .cursor_pointer()
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .child(match path {
            Some(path) => img(path)
                .size_full()
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
            None => div().size_full().into_any_element(),
        })
        .on_click(move |_, _, cx| {
            app.update(cx, |state, cx| state.set_call_background(pick.clone(), cx))
        })
        .into_any_element()
}

fn effect_change(
    app: &Entity<AppState>,
    change: fn(&mut CameraEffects),
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let app = app.clone();
    move |_, _, cx| {
        app.update(cx, |state, cx| {
            let mut effects = state.call_effects;
            change(&mut effects);
            state.set_call_effects(effects, cx);
        })
    }
}

fn effect_switch_item(
    app: &Entity<AppState>,
    id: &'static str,
    label: &'static str,
    on: bool,
    change: fn(&mut CameraEffects),
) -> PopupMenuItem {
    PopupMenuItem::element(move |_, _| {
        h_flex()
            .id(id)
            .w_full()
            .gap(px(16.))
            .items_center()
            .justify_between()
            .child(label)
            .child(Switch::new(id).checked(on))
    })
    .on_click(effect_change(app, change))
}

pub fn effect_items(popup: PopupMenu, app: &Entity<AppState>, effects: CameraEffects) -> PopupMenu {
    popup
        .separator()
        .item(PopupMenuItem::label("Effects"))
        .item(effect_switch_item(
            app,
            "call-effect-mirror",
            "Mirror my video",
            effects.mirror,
            |effects| effects.mirror = !effects.mirror,
        ))
        .item(effect_switch_item(
            app,
            "call-effect-framing",
            "Auto framing",
            effects.auto_framing,
            |effects| effects.auto_framing = !effects.auto_framing,
        ))
        .item(effect_switch_item(
            app,
            "call-effect-brightness",
            "Adjust brightness",
            effects.adjust_brightness,
            |effects| effects.adjust_brightness = !effects.adjust_brightness,
        ))
        .item(effect_switch_item(
            app,
            "call-effect-soft-focus",
            "Soft focus",
            effects.soft_focus,
            |effects| effects.soft_focus = !effects.soft_focus,
        ))
}

pub fn background_items(popup: PopupMenu, app: &Entity<AppState>, menu: &CameraMenu) -> PopupMenu {
    let (none_app, blur_app, add_app, grid_app) =
        (app.clone(), app.clone(), app.clone(), app.clone());
    let grid_menu = menu.clone();
    let mut popup = popup
        .separator()
        .item(PopupMenuItem::label("Background"))
        .item(
            PopupMenuItem::new("None")
                .checked(menu.pick == BackgroundPick::None)
                .on_click(move |_, _, cx| {
                    none_app.update(cx, |state, cx| {
                        state.set_call_background(BackgroundPick::None, cx)
                    })
                }),
        )
        .item(
            PopupMenuItem::new("Blur")
                .checked(menu.pick.is_blur())
                .on_click(move |_, _, cx| {
                    blur_app.update(cx, |state, cx| {
                        state.set_call_background(BackgroundPick::Blur, cx)
                    })
                }),
        );
    if !menu.images.is_empty() || !menu.customs.is_empty() {
        popup = popup.item(PopupMenuItem::element(move |_, _| {
            let defaults = grid_menu
                .images
                .iter()
                .enumerate()
                .map(|(index, (id, name, path))| {
                    let pick = BackgroundPick::Default(id.clone());
                    thumbnail(
                        &grid_app,
                        index,
                        name.clone(),
                        path.clone(),
                        grid_menu.pick == pick,
                        pick,
                    )
                });
            let customs = grid_menu.customs.iter().enumerate().map(|(index, path)| {
                let pick = BackgroundPick::Custom(path.clone());
                let name = path
                    .file_name()
                    .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
                thumbnail(
                    &grid_app,
                    grid_menu.images.len() + index,
                    name,
                    Some(path.clone()),
                    grid_menu.pick == pick,
                    pick,
                )
            });
            h_flex()
                .w(px(MENU_WIDTH - MENU_PADDING))
                .flex_wrap()
                .gap(px(THUMBNAIL_GAP))
                .children(defaults.chain(customs))
        }));
    }
    popup.item(
        PopupMenuItem::new("Add image...")
            .icon(IconName::Plus)
            .on_click(move |_, _, cx| {
                add_app.update(cx, |state, cx| state.add_call_background(cx))
            }),
    )
}
