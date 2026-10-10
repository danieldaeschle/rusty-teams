use std::time::{Duration, Instant};

use calling::DeviceEntry;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::avatar::person_avatar;
use super::widgets::icon;
use crate::app_state::AppState;
use crate::call::CallModel;
use crate::theme;

const HEADER_HEIGHT: f32 = 60.;
const TILE_WIDTH: f32 = 260.;
const TILE_HEIGHT: f32 = 210.;
const AVATAR_SIZE: f32 = 96.;
const RING_WIDTH: f32 = 3.;
const RING_GAP: f32 = 4.;
const CONTROL_SIZE: f32 = 44.;
const CONTROL_ICON: f32 = 20.;
const MENU_WIDTH: f32 = 280.;
const PULSE_PERIOD: Duration = Duration::from_millis(1400);
const DISABLED_OPACITY: f32 = 0.4;
pub const ECHO_NAME: &str = "Teams echo";
pub const NO_MICROPHONE_HINT: &str = "No microphone found";

pub fn status_line(model: &CallModel, now: Instant) -> (String, Hsla) {
    if model.is_connecting() {
        ("Calling...".to_owned(), theme::text_muted())
    } else if model.is_reconnecting() {
        ("Reconnecting...".to_owned(), theme::amber())
    } else {
        (model.timer_text(now), theme::text_muted())
    }
}

pub fn pulsing(element: impl IntoElement + 'static, id: &'static str) -> AnyElement {
    div()
        .child(element)
        .with_animation(
            id,
            Animation::new(PULSE_PERIOD).repeat(),
            |element, delta| {
                let wave = 1. - (delta * 2. - 1.).abs();
                element.opacity(0.45 + 0.55 * wave)
            },
        )
        .into_any_element()
}

fn speaking_ring(avatar: AnyElement, speaking: bool) -> Div {
    div()
        .p(px(RING_GAP))
        .rounded_full()
        .border(px(RING_WIDTH))
        .border_color(if speaking {
            theme::green()
        } else {
            transparent_black()
        })
        .child(avatar)
}

fn tile(avatar: AnyElement, name: &str, speaking: bool, muted: bool, caption: Option<AnyElement>) -> Div {
    let badge = div()
        .absolute()
        .top(px(10.))
        .right(px(10.))
        .size(px(24.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(theme::red())
        .child(icon(IconName::MicOff, 13., theme::white()));
    v_flex()
        .relative()
        .w(px(TILE_WIDTH))
        .h(px(TILE_HEIGHT))
        .items_center()
        .justify_center()
        .gap(px(12.))
        .rounded(px(12.))
        .bg(theme::surface())
        .border_1()
        .border_color(theme::border())
        .child(speaking_ring(avatar, speaking))
        .child(
            v_flex()
                .items_center()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(name.to_owned()),
                )
                .children(caption),
        )
        .when(muted, |tile| tile.child(badge))
}

fn round_control(id: &'static str, glyph: IconName, background: Hsla, foreground: Hsla) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(CONTROL_SIZE))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(background)
        .cursor_pointer()
        .hover(|button| button.opacity(0.85))
        .child(icon(glyph, CONTROL_ICON, foreground))
}

pub fn mute_button(app: &Entity<AppState>, model: &CallModel, id: &'static str, size: f32) -> Stateful<Div> {
    let (glyph, background, foreground) = if model.muted {
        (IconName::MicOff, theme::red(), theme::white())
    } else {
        (IconName::Mic, theme::surface_raised(), theme::text())
    };
    let button = round_control(id, glyph, background, foreground).size(px(size));
    if !model.can_unmute() {
        return button
            .opacity(DISABLED_OPACITY)
            .cursor_default()
            .tooltip(|window, cx| Tooltip::new(NO_MICROPHONE_HINT).build(window, cx));
    }
    let app = app.clone();
    button.on_click(move |_, _, cx| {
        cx.stop_propagation();
        app.update(cx, |state, cx| state.toggle_call_mute(cx));
    })
}

pub fn leave_button(app: &Entity<AppState>, model: &CallModel, id: &'static str, compact: bool) -> Stateful<Div> {
    let label = if model.is_connecting() { "Cancel" } else { "Leave" };
    let app = app.clone();
    h_flex()
        .id(id)
        .h(px(if compact { 28. } else { CONTROL_SIZE }))
        .px(px(if compact { 10. } else { 20. }))
        .gap(px(8.))
        .flex_none()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(theme::red())
        .text_color(theme::white())
        .text_size(px(if compact { 12. } else { 14. }))
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .hover(|button| button.opacity(0.85))
        .child(icon(IconName::PhoneOff, if compact { 14. } else { 18. }, theme::white()))
        .child(label)
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            app.update(cx, |state, cx| state.leave_call(cx));
        })
}

fn device_items(
    entries: Vec<DeviceEntry>,
    selected: &calling::DeviceChoice,
    pick: impl Fn(calling::DeviceChoice, &mut App) + Clone + 'static,
) -> Vec<PopupMenuItem> {
    entries
        .into_iter()
        .map(|entry| {
            let pick = pick.clone();
            let choice = entry.choice.clone();
            PopupMenuItem::new(entry.label)
                .checked(&entry.choice == selected)
                .on_click(move |_, _, cx| pick(choice.clone(), cx))
        })
        .collect()
}

fn devices_menu(popup: PopupMenu, app: &Entity<AppState>, model: &CallModel) -> PopupMenu {
    let input_app = app.clone();
    let output_app = app.clone();
    let inputs = device_items(model.devices.input_entries(), &model.input, move |choice, cx| {
        input_app.update(cx, |state, cx| state.select_call_input(choice, cx));
    });
    let outputs = device_items(model.devices.output_entries(), &model.output, move |choice, cx| {
        output_app.update(cx, |state, cx| state.select_call_output(choice, cx));
    });
    let mut popup = popup
        .min_w(px(MENU_WIDTH))
        .item(PopupMenuItem::label("Microphone"));
    if model.listen_only {
        popup = popup.item(PopupMenuItem::new(NO_MICROPHONE_HINT).disabled(true));
    } else {
        for item in inputs {
            popup = popup.item(item);
        }
    }
    popup = popup.separator().item(PopupMenuItem::label("Speaker"));
    for item in outputs {
        popup = popup.item(item);
    }
    popup
}

fn devices_button(app: &Entity<AppState>, model: &CallModel) -> impl IntoElement {
    let app = app.clone();
    let model = model.clone();
    Button::new("call-devices")
        .ghost()
        .p_0()
        .size(px(CONTROL_SIZE))
        .rounded_full()
        .child(round_control(
            "call-devices-face",
            IconName::Volume2,
            theme::surface_raised(),
            theme::text(),
        ))
        .dropdown_menu_with_anchor(Anchor::BottomLeft, move |popup, _, _| {
            devices_menu(popup, &app, &model)
        })
}

pub fn render_call_view(app: &Entity<AppState>, state: &AppState) -> Option<AnyElement> {
    let call = state.call.as_ref()?;
    let model = &call.model;
    let now = Instant::now();
    let (status, status_color) = status_line(model, now);
    let me = state.directory.me.as_ref();
    let header = h_flex()
        .w_full()
        .h(px(HEADER_HEIGHT))
        .flex_none()
        .px(px(20.))
        .gap(px(12.))
        .items_center()
        .border_b_1()
        .border_color(theme::border())
        .child(icon(IconName::Phone, 18., theme::accent_text()))
        .child(
            div()
                .text_size(px(16.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(model.title.clone()),
        )
        .child(div().text_size(px(13.)).text_color(status_color).child(status));

    let echo_avatar = person_avatar(&state.directory, None, ECHO_NAME, AVATAR_SIZE);
    let calling_caption = model.is_connecting().then(|| {
        pulsing(
            div()
                .text_size(px(12.))
                .text_color(theme::text_muted())
                .child("Calling..."),
            "call-calling",
        )
    });
    let echo_avatar = if model.is_connecting() {
        pulsing(echo_avatar, "call-echo-pulse")
    } else {
        echo_avatar
    };
    let echo_tile = tile(echo_avatar, ECHO_NAME, model.remote_speaking, false, calling_caption);
    let own_avatar = person_avatar(
        &state.directory,
        me.map(|person| person.user_id.as_str()),
        me.map_or("You", |person| person.display_name.as_str()),
        AVATAR_SIZE,
    );
    let own_tile = tile(own_avatar, "You", model.local_speaking, model.muted, None);

    let controls = h_flex()
        .w_full()
        .flex_none()
        .py(px(18.))
        .gap(px(12.))
        .items_center()
        .justify_center()
        .border_t_1()
        .border_color(theme::border())
        .child(mute_button(app, model, "call-mute", CONTROL_SIZE))
        .child(devices_button(app, model))
        .child(leave_button(app, model, "call-leave", false));

    Some(
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(theme::background())
            .child(header)
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .gap(px(20.))
                    .items_center()
                    .justify_center()
                    .child(echo_tile)
                    .child(own_tile),
            )
            .child(controls)
            .into_any_element(),
    )
}
