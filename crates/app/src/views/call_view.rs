use std::sync::Arc;
use std::time::{Duration, Instant};

use calling::{DeviceEntry, VideoKey};
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
use super::call_stage::render_stage;
use super::widgets::icon;
use crate::app_state::AppState;
use crate::call::TileSize;
use crate::call::{CallKind, CallModel, GridLayout, Tile, TileState, grid_layout, tile_size};
use crate::theme;

const HEADER_HEIGHT: f32 = 60.;
const RING_WIDTH: f32 = 3.;
const RING_GAP: f32 = 4.;
const CONTROL_SIZE: f32 = 44.;
const CONTROL_ICON: f32 = 20.;
const MENU_WIDTH: f32 = 280.;
const PULSE_PERIOD: Duration = Duration::from_millis(1400);
const DISABLED_OPACITY: f32 = 0.4;
const TILE_GAP: f32 = 20.;
const GRID_PADDING: f32 = 24.;
const STRIP_TILE: TileSize = TileSize {
    width: 232.,
    height: 131.,
    avatar: 56.,
};
const STRIP_GAP: f32 = 10.;
const STRIP_VISIBLE: f32 = 4.;
const TILE_RADIUS: f32 = 12.;
const NAME_PLATE_BACKGROUND: f32 = 0.6;
pub const NO_MICROPHONE_HINT: &str = "No microphone found";
pub const NO_CAMERA_HINT: &str = "No camera found";
pub const NO_SHARE_HINT: &str = "Sharing starts once the call is connected";
pub const NO_WINDOWS_HINT: &str = "No windows found";
const BANNER_HEIGHT: f32 = 40.;
const MAX_WINDOW_ITEMS: usize = 12;
pub const LOBBY_TEXT: &str = "Waiting in the lobby...";

pub fn status_line(model: &CallModel, now: Instant) -> (String, Hsla) {
    if model.lobby {
        (LOBBY_TEXT.to_owned(), theme::amber())
    } else if model.is_connecting() {
        (model.connecting_text().to_owned(), theme::text_muted())
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

fn mute_badge() -> Div {
    div()
        .absolute()
        .top(px(10.))
        .right(px(10.))
        .size(px(24.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(theme::red())
        .child(icon(IconName::MicOff, 13., theme::white()))
}

fn video_tile(size: TileSize, image: Arc<RenderImage>, name: &str, speaking: bool, muted: bool) -> Div {
    div()
        .relative()
        .w(px(size.width))
        .h(px(size.height))
        .flex_none()
        .overflow_hidden()
        .rounded(px(TILE_RADIUS))
        .bg(theme::surface())
        .border_2()
        .border_color(if speaking { theme::green() } else { transparent_black() })
        .child(
            img(ImageSource::Render(image))
                .size_full()
                .object_fit(ObjectFit::Cover)
                .rounded(px(TILE_RADIUS)),
        )
        .child(
            div()
                .absolute()
                .left(px(8.))
                .bottom(px(8.))
                .max_w(px(size.width - 24.))
                .truncate()
                .px(px(8.))
                .py(px(3.))
                .rounded(px(6.))
                .bg(hsla(0., 0., 0., NAME_PLATE_BACKGROUND))
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::white())
                .child(name.to_owned()),
        )
        .when(muted, |tile| tile.child(mute_badge()))
}

fn tile(
    size: TileSize,
    avatar: AnyElement,
    name: &str,
    speaking: bool,
    muted: bool,
    caption: Option<AnyElement>,
) -> Div {
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
        .w(px(size.width))
        .h(px(size.height))
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
                .max_w(px(size.width - 16.))
                .child(
                    div()
                        .max_w_full()
                        .truncate()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(name.to_owned()),
                )
                .children(caption),
        )
        .when(muted, |tile| tile.child(badge))
}

fn overflow_tile(size: TileSize, hidden: usize) -> Div {
    v_flex()
        .w(px(size.width))
        .h(px(size.height))
        .items_center()
        .justify_center()
        .rounded(px(12.))
        .bg(theme::surface())
        .border_1()
        .border_color(theme::border())
        .child(
            div()
                .text_size(px(size.avatar * 0.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text_muted())
                .child(format!("+{hidden}")),
        )
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

fn camera_button(app: &Entity<AppState>, model: &CallModel) -> Stateful<Div> {
    let (glyph, background, foreground) = if model.camera_on {
        (IconName::Video, theme::accent(), theme::on_accent())
    } else {
        (IconName::VideoOff, theme::surface_raised(), theme::text())
    };
    let button = round_control("call-camera", glyph, background, foreground);
    if !model.can_use_camera() {
        return button
            .opacity(DISABLED_OPACITY)
            .cursor_default()
            .tooltip(|window, cx| Tooltip::new(NO_CAMERA_HINT).build(window, cx));
    }
    let app = app.clone();
    button.on_click(move |_, _, cx| {
        cx.stop_propagation();
        app.update(cx, |state, cx| state.toggle_call_camera(cx));
    })
}

fn share_items(popup: PopupMenu, app: &Entity<AppState>, model: &CallModel) -> PopupMenu {
    let screens = model.screens();
    let single_screen = screens.len() == 1;
    let mut popup = popup.min_w(px(MENU_WIDTH));
    for (index, source) in screens.into_iter().enumerate() {
        let label = if single_screen { "Share entire screen".to_owned() } else { format!("Share screen {}", index + 1) };
        let source = source.clone();
        let app = app.clone();
        popup = popup.item(
            PopupMenuItem::new(label)
                .icon(IconName::Monitor)
                .on_click(move |_, _, cx| app.update(cx, |state, cx| state.start_call_share(source.clone(), cx))),
        );
    }
    popup = popup.separator().item(PopupMenuItem::label("Share a window..."));
    let windows = model.windows();
    if windows.is_empty() {
        return popup.item(PopupMenuItem::new(NO_WINDOWS_HINT).disabled(true));
    }
    for source in windows.into_iter().take(MAX_WINDOW_ITEMS) {
        let label = source.label();
        let source = source.clone();
        let app = app.clone();
        popup = popup.item(
            PopupMenuItem::new(label)
                .icon(IconName::AppWindow)
                .on_click(move |_, _, cx| app.update(cx, |state, cx| state.start_call_share(source.clone(), cx))),
        );
    }
    popup
}

fn share_button(app: &Entity<AppState>, model: &CallModel) -> AnyElement {
    if model.local_share.is_some() {
        let app = app.clone();
        return round_control("call-share-stop", IconName::ScreenShareOff, theme::accent(), theme::on_accent())
            .tooltip(|window, cx| Tooltip::new("Stop sharing").build(window, cx))
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                app.update(cx, |state, cx| state.stop_call_share(cx));
            })
            .into_any_element();
    }
    let face = round_control("call-share-face", IconName::ScreenShare, theme::surface_raised(), theme::text());
    if !model.can_share() {
        return face
            .opacity(DISABLED_OPACITY)
            .cursor_default()
            .tooltip(|window, cx| Tooltip::new(NO_SHARE_HINT).build(window, cx))
            .into_any_element();
    }
    let app = app.clone();
    let model = model.clone();
    Button::new("call-share")
        .ghost()
        .p_0()
        .size(px(CONTROL_SIZE))
        .rounded_full()
        .child(face)
        .dropdown_menu_with_anchor(Anchor::BottomLeft, move |popup, _, _| share_items(popup, &app, &model))
        .into_any_element()
}

fn sharing_banner(app: &Entity<AppState>, model: &CallModel) -> Option<AnyElement> {
    let label = model.local_share.as_ref()?;
    let app = app.clone();
    Some(
        h_flex()
            .id("call-sharing-banner")
            .w_full()
            .h(px(BANNER_HEIGHT))
            .flex_none()
            .px(px(20.))
            .gap(px(12.))
            .items_center()
            .justify_center()
            .bg(theme::accent())
            .text_color(theme::on_accent())
            .text_size(px(13.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(icon(IconName::ScreenShare, 16., theme::on_accent()))
            .child(format!("You are sharing: {label}"))
            .child(
                div()
                    .id("call-sharing-stop")
                    .h(px(26.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .rounded_full()
                    .bg(theme::on_accent())
                    .text_color(theme::accent())
                    .cursor_pointer()
                    .hover(|button| button.opacity(0.9))
                    .child("Stop sharing")
                    .on_click(move |_, _, cx| app.update(cx, |state, cx| state.stop_call_share(cx))),
            )
            .into_any_element(),
    )
}

fn chat_button(app: &Entity<AppState>) -> Stateful<Div> {
    let app = app.clone();
    round_control("call-chat", IconName::MessageCircle, theme::surface_raised(), theme::text())
        .tooltip(|window, cx| Tooltip::new("Open the chat").build(window, cx))
        .on_click(move |_, _, cx| app.update(cx, |state, cx| state.open_call_chat(cx)))
}

fn leave_menu(app: &Entity<AppState>, model: &CallModel) -> impl IntoElement {
    let leave_app = app.clone();
    let end_app = app.clone();
    let can_end = model.can_end_meeting;
    Button::new("call-leave-menu")
        .ghost()
        .p_0()
        .size(px(CONTROL_SIZE))
        .rounded_full()
        .child(round_control(
            "call-leave-menu-face",
            IconName::ChevronUp,
            theme::surface_raised(),
            theme::text(),
        ))
        .dropdown_menu_with_anchor(Anchor::BottomRight, move |popup, _, _| {
            let leave_app = leave_app.clone();
            let end_app = end_app.clone();
            let popup = popup.min_w(px(MENU_WIDTH)).item(
                PopupMenuItem::new("Leave")
                    .icon(IconName::PhoneOff)
                    .on_click(move |_, _, cx| leave_app.update(cx, |state, cx| state.leave_call(cx))),
            );
            if can_end {
                popup.item(
                    PopupMenuItem::new("End meeting for everyone")
                        .icon(IconName::PhoneOff)
                        .on_click(move |_, _, cx| end_app.update(cx, |state, cx| state.end_meeting_for_all(cx))),
                )
            } else {
                popup
            }
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
    if model.can_use_camera() {
        let camera_app = app.clone();
        let cameras = device_items(calling::camera_entries(&model.cameras), &model.camera, move |choice, cx| {
            camera_app.update(cx, |state, cx| state.select_call_camera(choice, cx));
        });
        popup = popup.separator().item(PopupMenuItem::label("Camera"));
        for item in cameras {
            popup = popup.item(item);
        }
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

fn person_tile(state: &AppState, model: &CallModel, remote: &Tile, size: TileSize, now: Instant) -> AnyElement {
    let video = state
        .call
        .as_ref()
        .filter(|_| remote.has_video)
        .and_then(|call| call.pictures.live(&VideoKey::Person(remote.mri.clone()), now));
    if let Some(image) = video {
        return video_tile(size, image, &remote.name, model.tile_speaking(remote), remote.muted).into_any_element();
    }
    let waiting = remote.state == TileState::Invited;
    let avatar = person_avatar(&state.directory, remote.user_id.as_deref(), &remote.name, size.avatar);
    let avatar = if waiting { pulsing(avatar, "call-invited-pulse") } else { avatar };
    let caption = model.tile_caption(remote).map(|text| {
        let label = div().text_size(px(12.)).text_color(theme::text_muted()).child(text);
        if waiting { pulsing(label, "call-invited-caption") } else { label.into_any_element() }
    });
    tile(size, avatar, &remote.name, model.tile_speaking(remote), remote.muted, caption).into_any_element()
}

fn own_tile(state: &AppState, model: &CallModel, size: TileSize, now: Instant) -> AnyElement {
    let me = state.directory.me.as_ref();
    let self_view = state
        .call
        .as_ref()
        .and_then(|call| call.pictures.live(&VideoKey::LocalCamera, now));
    if let Some(image) = self_view {
        return video_tile(size, image, "You", model.local_speaking, model.muted).into_any_element();
    }
    let avatar = person_avatar(
        &state.directory,
        me.map(|person| person.user_id.as_str()),
        me.map_or("You", |person| person.display_name.as_str()),
        size.avatar,
    );
    tile(size, avatar, "You", model.local_speaking, model.muted, None).into_any_element()
}

fn remote_tiles(state: &AppState, model: &CallModel, size: TileSize, hidden_cap: GridLayout, now: Instant) -> Vec<AnyElement> {
    let visible = model.visible_tiles();
    let mut cells: Vec<AnyElement> = visible
        .iter()
        .take(hidden_cap.shown_others)
        .map(|remote| person_tile(state, model, remote, size, now))
        .collect();
    if hidden_cap.hidden > 0 {
        cells.push(overflow_tile(size, hidden_cap.hidden).into_any_element());
    }
    cells
}

fn people_strip(state: &AppState, model: &CallModel, now: Instant) -> Stateful<Div> {
    let mut cells: Vec<AnyElement> = model
        .strip_tiles()
        .into_iter()
        .map(|remote| person_tile(state, model, remote, STRIP_TILE, now))
        .collect();
    cells.push(own_tile(state, model, STRIP_TILE, now));
    v_flex()
        .id("call-strip")
        .flex_none()
        .w(px(STRIP_TILE.width))
        .max_h(px(STRIP_VISIBLE * (STRIP_TILE.height + STRIP_GAP) - STRIP_GAP))
        .gap(px(STRIP_GAP))
        .overflow_y_scroll()
        .children(cells)
}

pub fn render_call_view(app: &Entity<AppState>, state: &AppState) -> Option<AnyElement> {
    let call = state.call.as_ref()?;
    let model = &call.model;
    let now = Instant::now();
    let (status, status_color) = status_line(model, now);
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
                .min_w_0()
                .truncate()
                .text_size(px(16.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(model.title.clone()),
        )
        .child(div().flex_none().text_size(px(13.)).text_color(status_color).child(status));

    let layout = grid_layout(model.visible_tiles().len());
    let size = tile_size(layout.shown_others + usize::from(layout.hidden > 0) + 1);
    let mut cells = remote_tiles(state, model, size, layout, now);
    cells.push(own_tile(state, model, size, now));
    let stage = render_stage(app, state, false);

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
        .child(camera_button(app, model))
        .child(share_button(app, model))
        .child(devices_button(app, model))
        .when(call.conversation_id.is_some(), |row| row.child(chat_button(app)))
        .child(leave_button(app, model, "call-leave", false))
        .when(model.kind == CallKind::Meeting, |row| row.child(leave_menu(app, model)));

    Some(
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(theme::background())
            .child(header)
            .children(sharing_banner(app, model))
            .child(match stage {
                Some(stage) => h_flex()
                    .flex_1()
                    .min_h_0()
                    .p(px(GRID_PADDING))
                    .gap(px(TILE_GAP))
                    .items_center()
                    .child(stage)
                    .child(people_strip(state, model, now))
                    .into_any_element(),
                None => h_flex()
                    .flex_1()
                    .min_h_0()
                    .flex_wrap()
                    .content_center()
                    .p(px(GRID_PADDING))
                    .gap(px(TILE_GAP))
                    .items_center()
                    .justify_center()
                    .children(cells)
                    .into_any_element(),
            })
            .child(controls)
            .into_any_element(),
    )
}
