use std::sync::Arc;
use std::time::Instant;

use calling::{CaptionState, HoldState, VideoKey};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::avatar::person_avatar;
use super::call_extras::{BackgroundMenu, background_items, confirm_stop_recording, open_transfer_picker};
use super::call_view::{BANNER_HEIGHT, CONTROL_SIZE, MENU_WIDTH, round_control};
use super::widgets::icon;
use crate::app_state::AppState;
use crate::call::{CAPTION_SHOWN, CallModel, Focus};
use crate::notice::truncated;
use crate::theme;

const BANNER_TEXT: f32 = 13.;
const PILL_HEIGHT: f32 = 26.;
const LOBBY_LIST_WIDTH: f32 = 340.;
const LOBBY_NAME_LIMIT: usize = 28;
const CAPTION_WIDTH: f32 = 720.;
const CAPTION_BOTTOM: f32 = 2. * 18. + CONTROL_SIZE + 1. + 16.;
const CAPTION_FADE_SECONDS: f32 = 1.;
const CAPTION_BACKGROUND: f32 = 0.72;
const FOCUS_RADIUS: f32 = 12.;
const FOCUS_AVATAR: f32 = 140.;
const CHIP_HEIGHT: f32 = 24.;
const MORE_SIZE: f32 = 28.;
const SMALL_CONTROL: f32 = 28.;
const NAME_LIMIT: usize = 32;

fn dark_text() -> Hsla {
    hsla(0., 0., 0.08, 1.)
}

pub(super) fn pill(id: impl Into<ElementId>, label: &'static str, filled: bool) -> Stateful<Div> {
    let (background, foreground) = if filled { (dark_text(), theme::white()) } else { (transparent_black(), dark_text()) };
    div()
        .id(id)
        .h(px(PILL_HEIGHT))
        .px(px(12.))
        .flex()
        .flex_none()
        .items_center()
        .rounded_full()
        .border_1()
        .border_color(dark_text())
        .bg(background)
        .text_color(foreground)
        .text_size(px(BANNER_TEXT))
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .hover(|button| button.opacity(0.85))
        .child(label)
}

pub fn lobby_text(waiting: usize) -> String {
    match waiting {
        1 => "1 person is waiting in the lobby".to_owned(),
        count => format!("{count} people are waiting in the lobby"),
    }
}

fn lobby_list(app: &Entity<AppState>, guests: &[(String, String)]) -> Div {
    v_flex().w(px(LOBBY_LIST_WIDTH)).gap(px(6.)).children(guests.iter().map(|(mri, name)| {
        let (admit_app, deny_app) = (app.clone(), app.clone());
        let (admit_mri, deny_mri) = (mri.clone(), mri.clone());
        h_flex()
            .w_full()
            .gap(px(8.))
            .items_center()
            .child(div().flex_1().min_w_0().truncate().text_size(px(14.)).child(truncated(name, LOBBY_NAME_LIMIT)))
            .child(
                Button::new(SharedString::from(format!("lobby-admit-{mri}")))
                    .label("Admit")
                    .on_click(move |_, _, cx| admit_app.update(cx, |state, cx| state.admit_call_guest(admit_mri.clone(), cx))),
            )
            .child(
                Button::new(SharedString::from(format!("lobby-deny-{mri}")))
                    .label("Deny")
                    .ghost()
                    .on_click(move |_, _, cx| deny_app.update(cx, |state, cx| state.deny_call_guest(deny_mri.clone(), cx))),
            )
    }))
}

pub fn lobby_banner(app: &Entity<AppState>, model: &CallModel) -> Option<AnyElement> {
    if !model.can_admit() {
        return None;
    }
    let guests: Vec<(String, String)> = model.lobby_guests().into_iter().map(|tile| (tile.mri.clone(), tile.name.clone())).collect();
    if guests.is_empty() {
        return None;
    }
    let list_app = app.clone();
    let admit_app = app.clone();
    Some(
        h_flex()
            .id("call-lobby-banner")
            .w_full()
            .h(px(BANNER_HEIGHT))
            .flex_none()
            .px(px(20.))
            .gap(px(12.))
            .items_center()
            .justify_center()
            .bg(theme::amber())
            .text_color(dark_text())
            .text_size(px(BANNER_TEXT))
            .font_weight(FontWeight::SEMIBOLD)
            .child(lobby_text(guests.len()))
            .child(
                Popover::new("call-lobby-popover")
                    .anchor(Anchor::TopLeft)
                    .offset(px(8.))
                    .trigger(Button::new("call-lobby-view").ghost().p_0().h(px(PILL_HEIGHT)).rounded_full().child(pill("call-lobby-view-face", "View", false)))
                    .content(move |_, _, _| lobby_list(&list_app, &guests)),
            )
            .child(
                pill("call-lobby-admit-all", "Admit all", true)
                    .on_click(move |_, _, cx| admit_app.update(cx, |state, cx| state.admit_all_call_guests(cx))),
            )
            .into_any_element(),
    )
}

fn confirm_remove(app: &Entity<AppState>, mri: &str, name: &str, window: &mut Window, cx: &mut App) {
    let app = app.clone();
    let mri = mri.to_owned();
    let heading = format!("Remove {} from the meeting?", truncated(name, NAME_LIMIT));
    window.open_alert_dialog(cx, move |alert, _, _| {
        let app = app.clone();
        let mri = mri.clone();
        let cancel = Button::new("remove-cancel").label("Cancel").on_click(|_, window, cx| window.close_dialog(cx));
        let remove = Button::new("remove-confirm").label("Remove").danger().on_click(move |_, window, cx| {
            app.update(cx, |state, cx| state.remove_call_participant(mri.clone(), cx));
            window.close_dialog(cx);
        });
        alert
            .title(heading.clone())
            .description("They leave the call and can only come back if they join again.")
            .footer(DialogFooter::new().child(cancel).child(remove))
            .on_ok(|_, _, _| false)
    });
}

pub fn tile_menu(popup: PopupMenu, app: &Entity<AppState>, model: &CallModel, mri: &str, name: &str, own: bool) -> PopupMenu {
    let pinned = model.is_pinned(mri);
    let spotlighted = model.is_spotlighted(mri);
    let organizes = model.organizes_meeting();
    let raised = !own && model.tiles.iter().any(|tile| tile.mri == mri && tile.hand.is_some());
    let muted = model.tiles.iter().any(|tile| tile.mri == mri && tile.muted);
    let mut popup = popup.min_w(px(MENU_WIDTH));
    let pin_app = app.clone();
    let pin_mri = mri.to_owned();
    popup = popup.item(
        PopupMenuItem::new(if pinned { "Unpin" } else { "Pin for me" })
            .icon(IconName::Frame)
            .on_click(move |_, _, cx| pin_app.update(cx, |state, cx| state.toggle_call_pin(&pin_mri, cx))),
    );
    if organizes {
        let (spotlight_app, spotlight_mri) = (app.clone(), mri.to_owned());
        popup = popup.item(
            PopupMenuItem::new(if spotlighted { "Stop spotlight" } else { "Spotlight for everyone" })
                .icon(IconName::Star)
                .on_click(move |_, _, cx| spotlight_app.update(cx, |state, cx| state.toggle_call_spotlight(spotlight_mri.clone(), cx))),
        );
    }
    if organizes && !own {
        let (mute_app, mute_mri) = (app.clone(), mri.to_owned());
        popup = popup.item(
            PopupMenuItem::new("Mute")
                .icon(IconName::MicOff)
                .disabled(muted)
                .on_click(move |_, _, cx| mute_app.update(cx, |state, cx| state.mute_call_participant(mute_mri.clone(), cx))),
        );
    }
    if organizes && raised {
        let (hand_app, hand_mri) = (app.clone(), mri.to_owned());
        popup = popup.item(
            PopupMenuItem::new("Lower hand").on_click(move |_, _, cx| hand_app.update(cx, |state, cx| state.lower_call_hand(hand_mri.clone(), cx))),
        );
    }
    if organizes && !own {
        let (remove_app, remove_mri, remove_name) = (app.clone(), mri.to_owned(), name.to_owned());
        popup = popup.separator().item(
            PopupMenuItem::new("Remove from meeting...")
                .icon(IconName::Close)
                .on_click(move |_, window, cx| confirm_remove(&remove_app, &remove_mri, &remove_name, window, cx)),
        );
    }
    popup
}

fn more_overlay(app: &Entity<AppState>, model: &CallModel, mri: &str, name: &str, own: bool, group: SharedString) -> Div {
    let (app, model, mri_owned, name) = (app.clone(), model.clone(), mri.to_owned(), name.to_owned());
    div().absolute().top(px(10.)).right(px(38.)).opacity(0.).group_hover(group, |more| more.opacity(1.)).child(
        Button::new(SharedString::from(format!("call-tile-more-{mri}")))
            .icon(IconName::Ellipsis)
            .ghost()
            .xsmall()
            .size(px(MORE_SIZE))
            .rounded_full()
            .bg(black().opacity(0.55))
            .text_color(theme::white())
            .tooltip("More")
            .dropdown_menu_with_anchor(Anchor::TopRight, move |popup, _, _| tile_menu(popup, &app, &model, &mri_owned, &name, own)),
    )
}

/// Adds the right-click menu and a "..." button that shows while the pointer is over the tile.
pub fn attach_tile_menu(tile: Stateful<Div>, app: &Entity<AppState>, model: &CallModel, mri: &str, name: &str, own: bool) -> AnyElement {
    let group = SharedString::from(format!("call-tile-group-{mri}"));
    let (context_app, context_model, context_mri, context_name) = (app.clone(), model.clone(), mri.to_owned(), name.to_owned());
    tile.group(group.clone())
        .relative()
        .child(more_overlay(app, model, mri, name, own, group))
        .context_menu(move |popup, _, _| tile_menu(popup, &context_app, &context_model, &context_mri, &context_name, own))
        .into_any_element()
}

pub fn with_tile_menu(app: &Entity<AppState>, model: &CallModel, mri: &str, name: &str, own: bool, tile: AnyElement) -> AnyElement {
    let wrapper = div().id(SharedString::from(format!("call-tile-{mri}"))).flex_none().child(tile);
    attach_tile_menu(wrapper, app, model, mri, name, own)
}

pub fn more_button(app: &Entity<AppState>, model: &CallModel) -> Option<AnyElement> {
    let in_meeting = model.kind == crate::call::CallKind::Meeting;
    if !in_meeting && !model.is_one_to_one() {
        return None;
    }
    let app = app.clone();
    let model = model.clone();
    Some(
        Button::new("call-more")
            .ghost()
            .p_0()
            .size(px(CONTROL_SIZE))
            .rounded_full()
            .child(round_control("call-more-face", IconName::Ellipsis, theme::surface_raised(), theme::text()))
            .dropdown_menu_with_anchor(Anchor::BottomRight, move |popup, _, _| more_menu(popup, &app, &model))
            .into_any_element(),
    )
}

fn more_menu(popup: PopupMenu, app: &Entity<AppState>, model: &CallModel) -> PopupMenu {
    let popup = popup.min_w(px(MENU_WIDTH));
    if model.is_one_to_one() {
        return one_to_one_items(popup, app, model);
    }
    meeting_items(popup, app, model)
}

fn one_to_one_items(mut popup: PopupMenu, app: &Entity<AppState>, model: &CallModel) -> PopupMenu {
    let (hold_app, transfer_app) = (app.clone(), app.clone());
    let held = model.hold == HoldState::Local;
    popup = popup.item(
        PopupMenuItem::new(if held { "Resume" } else { "Hold" })
            .icon(IconName::Timer)
            .disabled(!held && !model.can_hold())
            .on_click(move |_, _, cx| hold_app.update(cx, |state, cx| state.toggle_call_hold(cx))),
    );
    let can_transfer = model.can_transfer();
    popup.item(
        PopupMenuItem::new("Transfer...")
            .icon(IconName::Forward)
            .disabled(!can_transfer)
            .on_click(move |_, window, cx| open_transfer_picker(&transfer_app, window, cx)),
    )
}

fn meeting_items(mut popup: PopupMenu, app: &Entity<AppState>, model: &CallModel) -> PopupMenu {
    let (mute_app, captions_app, record_app, board_app) = (app.clone(), app.clone(), app.clone(), app.clone());
    let captions_on = model.captions_on();
    if model.organizes_meeting() {
        popup = popup.item(
            PopupMenuItem::new("Mute all")
                .icon(IconName::MicOff)
                .on_click(move |_, _, cx| mute_app.update(cx, |state, cx| state.mute_all_call(cx))),
        );
    }
    if model.can_record() {
        let recording = model.recording;
        popup = popup.item(
            PopupMenuItem::new(if recording { "Stop recording" } else { "Start recording" })
                .icon(IconName::Video)
                .on_click(move |_, window, cx| {
                    if recording {
                        confirm_stop_recording(&record_app, window, cx);
                    } else {
                        record_app.update(cx, |state, cx| state.set_call_recording(true, cx));
                    }
                }),
        );
    }
    if model.can_open_whiteboard() {
        popup = popup.item(
            PopupMenuItem::new("Whiteboard")
                .icon(IconName::Frame)
                .on_click(move |_, _, cx| board_app.update(cx, |state, cx| state.open_call_whiteboard(cx))),
        );
    }
    popup.item(
        PopupMenuItem::new(if captions_on { "Turn off live captions" } else { "Turn on live captions" })
            .icon(if captions_on { IconName::CaptionsOff } else { IconName::Captions })
            .on_click(move |_, _, cx| captions_app.update(cx, |state, cx| state.toggle_call_captions(cx))),
    )
}

pub fn camera_menu_button(app: &Entity<AppState>, model: &CallModel, backgrounds: BackgroundMenu) -> Option<AnyElement> {
    if !model.can_use_camera() {
        return None;
    }
    let app = app.clone();
    let model = model.clone();
    Some(
        Button::new("call-camera-menu")
            .ghost()
            .p_0()
            .size(px(SMALL_CONTROL))
            .rounded_full()
            .child(
                div()
                    .id("call-camera-menu-face")
                    .size(px(SMALL_CONTROL))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(theme::surface_raised())
                    .child(icon(IconName::ChevronUp, 16., theme::text())),
            )
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |popup, _, _| camera_menu(popup, &app, &model, &backgrounds))
            .into_any_element(),
    )
}

fn camera_menu(popup: PopupMenu, app: &Entity<AppState>, model: &CallModel, backgrounds: &BackgroundMenu) -> PopupMenu {
    let camera_app = app.clone();
    let cameras = super::call_view::device_items(calling::camera_entries(&model.cameras), &model.camera, move |choice, cx| {
        camera_app.update(cx, |state, cx| state.select_call_camera(choice, cx));
    });
    let mut popup = popup.min_w(px(MENU_WIDTH)).item(PopupMenuItem::label("Camera"));
    for item in cameras {
        popup = popup.item(item);
    }
    background_items(popup, app, backgrounds)
}

pub fn captions_overlay(model: &CallModel, now: Instant) -> Option<AnyElement> {
    let lines = model.visible_captions(now);
    if lines.is_empty() {
        return None;
    }
    let shown = CAPTION_SHOWN.as_secs_f32();
    Some(
        div()
            .id("call-captions")
            .absolute()
            .left_0()
            .right_0()
            .bottom(px(CAPTION_BOTTOM))
            .flex()
            .justify_center()
            .px(px(24.))
            .child(
                v_flex()
                    .w(px(CAPTION_WIDTH))
                    .max_w_full()
                    .gap(px(4.))
                    .px(px(16.))
                    .py(px(10.))
                    .rounded(px(10.))
                    .bg(hsla(0., 0., 0., CAPTION_BACKGROUND))
                    .text_color(theme::white())
                    .text_size(px(16.))
                    .children(lines.into_iter().map(|line| {
                        let age = now.saturating_duration_since(line.at).as_secs_f32();
                        let fade = ((shown - age) / CAPTION_FADE_SECONDS).clamp(0., 1.);
                        h_flex()
                            .opacity(fade)
                            .gap(px(8.))
                            .items_start()
                            .child(div().flex_none().font_weight(FontWeight::BOLD).child(format!("{}:", line.speaker)))
                            .child(div().flex_1().min_w_0().child(line.text.clone()))
                    })),
            )
            .into_any_element(),
    )
}

fn focus_chip(label: &'static str, glyph: IconName, background: Hsla, foreground: Hsla) -> Div {
    h_flex()
        .h(px(CHIP_HEIGHT))
        .px(px(8.))
        .gap(px(4.))
        .items_center()
        .rounded_full()
        .bg(background)
        .text_color(foreground)
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(icon(glyph, 13., foreground))
        .child(label)
}

pub fn spotlight_chip() -> Div {
    focus_chip("Spotlight", IconName::Star, theme::amber(), dark_text())
}

pub fn pin_chip() -> Div {
    focus_chip("Pinned", IconName::Frame, hsla(0., 0., 0., 0.6), theme::white())
}

pub fn render_focus_stage(app: &Entity<AppState>, state: &AppState, focus: &Focus, now: Instant) -> Option<AnyElement> {
    let call = state.call.as_ref()?;
    let model = &call.model;
    let me = state.directory.me.as_ref();
    let (name, user_id, speaking, muted, key) = if focus.own {
        ("You".to_owned(), me.map(|person| person.user_id.clone()), model.local_speaking, model.muted, VideoKey::LocalCamera)
    } else {
        let tile = model.tiles.iter().find(|tile| tile.mri == focus.mri)?;
        (tile.name.clone(), tile.user_id.clone(), model.tile_speaking(tile), tile.muted, VideoKey::Person(tile.mri.clone()))
    };
    let image: Option<Arc<RenderImage>> = call.pictures.live(&key, now);
    let picture = match image {
        Some(image) => img(ImageSource::Render(image)).size_full().object_fit(ObjectFit::Contain).into_any_element(),
        None => div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(person_avatar(&state.directory, user_id.as_deref(), &name, FOCUS_AVATAR))
            .into_any_element(),
    };
    let stage = div()
        .id("call-focus-stage")
        .relative()
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_hidden()
        .rounded(px(FOCUS_RADIUS))
        .bg(hsla(0., 0., 0.04, 1.))
        .border_2()
        .border_color(if speaking { theme::green() } else { transparent_black() })
        .child(picture)
        .child(
            h_flex()
                .absolute()
                .top(px(14.))
                .left(px(14.))
                .gap(px(6.))
                .when(focus.spotlight, |chips| chips.child(spotlight_chip()))
                .when(focus.pinned, |chips| chips.child(pin_chip())),
        )
        .child(
            h_flex()
                .absolute()
                .left(px(14.))
                .bottom(px(14.))
                .gap(px(8.))
                .px(px(10.))
                .py(px(4.))
                .rounded(px(6.))
                .bg(hsla(0., 0., 0., 0.6))
                .text_color(theme::white())
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(name.clone())
                .when(muted, |plate| plate.child(icon(IconName::MicOff, 14., theme::red()))),
        );
    Some(attach_tile_menu(stage, app, model, &focus.mri, &name, focus.own))
}

pub fn captions_state_hint(model: &CallModel) -> Option<&'static str> {
    match model.captions {
        CaptionState::Starting => Some("Starting live captions..."),
        CaptionState::Failed(_) => Some("Live captions could not start"),
        CaptionState::On | CaptionState::Off => None,
    }
}

pub fn captions_hint_chip(model: &CallModel) -> Option<AnyElement> {
    let text = captions_state_hint(model)?;
    Some(
        div()
            .id("call-captions-hint")
            .absolute()
            .left_0()
            .right_0()
            .bottom(px(CAPTION_BOTTOM))
            .flex()
            .justify_center()
            .child(
                div()
                    .px(px(14.))
                    .py(px(6.))
                    .rounded_full()
                    .bg(hsla(0., 0., 0., CAPTION_BACKGROUND))
                    .text_color(theme::white())
                    .text_size(px(13.))
                    .child(text),
            )
            .into_any_element(),
    )
}
