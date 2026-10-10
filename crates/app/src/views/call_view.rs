use std::sync::Arc;
use std::time::{Duration, Instant};

use calling::{DeviceEntry, Reaction, VideoKey};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem},
    popover::Popover,
    switch::Switch,
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::avatar::person_avatar;
use super::call_stage::render_stage;
use super::conversation::ConversationView;
use super::widgets::{count_badge, icon, symbol};
use crate::app_state::AppState;
use crate::call::TileSize;
use crate::call::{CHAT_OPEN_TILES, CallKind, CallModel, GridLayout, MAX_TILES, Tile, TileState, grid_layout, tile_size};
use crate::call::REACTION_SHOWN;
use crate::rows::reaction_glyph as chat_reaction_glyph;
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
pub const CHAT_PANEL_WIDTH: f32 = 320.;
const CHAT_HEADER_HEIGHT: f32 = 48.;
const HAND_GLYPH: &str = "\u{270B}";
const REACTION_RISE: f32 = 24.;
const REACTION_FADE_FROM: f32 = 0.75;
const REACTION_CELL: f32 = 40.;
const SHARE_SOUND_LABEL: &str = "Include computer sound";

pub fn reaction_glyph(reaction: Reaction) -> String {
    match reaction {
        Reaction::Applause => "\u{1F44F}".to_owned(),
        other => chat_reaction_glyph(other.name()),
    }
}

#[derive(Default)]
struct TileExtras {
    hand: Option<usize>,
    reaction: Option<(Reaction, u64)>,
}

fn hand_badge(position: usize) -> Div {
    h_flex()
        .absolute()
        .top(px(10.))
        .left(px(10.))
        .h(px(24.))
        .px(px(8.))
        .gap(px(4.))
        .items_center()
        .rounded_full()
        .bg(theme::amber())
        .text_color(hsla(0., 0., 0.08, 1.))
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(format!("Hand {position}"))
}

fn reaction_chip(reaction: Reaction, serial: u64) -> AnyElement {
    div()
        .absolute()
        .right(px(12.))
        .bottom(px(34.))
        .size(px(36.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(theme::surface_raised())
        .border_1()
        .border_color(theme::border_strong())
        .text_size(px(20.))
        .child(reaction_glyph(reaction))
        .with_animation(
            ("call-reaction", serial as usize),
            Animation::new(REACTION_SHOWN),
            |chip, delta| {
                let fade = ((delta - REACTION_FADE_FROM) / (1. - REACTION_FADE_FROM)).clamp(0., 1.);
                chip.bottom(px(34. + REACTION_RISE * delta)).opacity(1. - fade)
            },
        )
        .into_any_element()
}

fn tile_overlays(extras: &TileExtras) -> Vec<AnyElement> {
    let hand = extras.hand.map(|position| hand_badge(position).into_any_element());
    let reaction = extras.reaction.map(|(reaction, serial)| reaction_chip(reaction, serial));
    hand.into_iter().chain(reaction).collect()
}

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

fn video_tile(size: TileSize, image: Arc<RenderImage>, name: &str, speaking: bool, muted: bool, extras: &TileExtras) -> Div {
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
        .children(tile_overlays(extras))
}

fn tile(
    size: TileSize,
    avatar: AnyElement,
    name: &str,
    speaking: bool,
    muted: bool,
    caption: Option<AnyElement>,
    extras: &TileExtras,
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
        .children(tile_overlays(extras))
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

fn control_shell(id: &'static str, background: Hsla) -> Stateful<Div> {
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
}

fn round_control(id: &'static str, glyph: IconName, background: Hsla, foreground: Hsla) -> Stateful<Div> {
    control_shell(id, background).child(icon(glyph, CONTROL_ICON, foreground))
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

fn sound_switch_item(app: &Entity<AppState>, sound: bool) -> PopupMenuItem {
    let app = app.clone();
    PopupMenuItem::element(move |_, _| {
        h_flex()
            .id("call-share-sound-row")
            .w_full()
            .gap(px(16.))
            .items_center()
            .justify_between()
            .child(SHARE_SOUND_LABEL)
            .child(Switch::new("call-share-sound").checked(sound))
    })
    .on_click(move |_, _, cx| app.update(cx, |state, cx| state.set_call_share_sound(!sound, cx)))
}

fn share_items(popup: PopupMenu, app: &Entity<AppState>, model: &CallModel, sound: bool) -> PopupMenu {
    let screens = model.screens();
    let single_screen = screens.len() == 1;
    let mut popup = popup.min_w(px(MENU_WIDTH)).item(sound_switch_item(app, sound)).separator();
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

fn share_button(app: &Entity<AppState>, model: &CallModel, sound: bool) -> AnyElement {
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
        .dropdown_menu_with_anchor(Anchor::BottomLeft, move |popup, _, _| share_items(popup, &app, &model, sound))
        .into_any_element()
}

fn sharing_banner(app: &Entity<AppState>, model: &CallModel, sound: bool) -> Option<AnyElement> {
    let label = model.local_share.as_ref()?;
    let sound_app = app.clone();
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
                    .id("call-sharing-sound")
                    .h(px(26.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .rounded_full()
                    .border_1()
                    .border_color(theme::on_accent())
                    .cursor_pointer()
                    .hover(|button| button.opacity(0.9))
                    .child(if sound { "Computer sound on" } else { "Computer sound off" })
                    .on_click(move |_, _, cx| sound_app.update(cx, |state, cx| state.set_call_share_sound(!sound, cx))),
            )
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

fn chat_button(app: &Entity<AppState>, open: bool, unread: u32) -> Stateful<Div> {
    let app = app.clone();
    let (background, foreground) = if open {
        (theme::accent(), theme::on_accent())
    } else {
        (theme::surface_raised(), theme::text())
    };
    round_control("call-chat", IconName::MessageCircle, background, foreground)
        .relative()
        .tooltip(move |window, cx| Tooltip::new(if open { "Close the chat" } else { "Open the chat" }).build(window, cx))
        .when(unread > 0, |button| {
            button.child(div().absolute().top(px(-4.)).right(px(-4.)).child(count_badge(unread, false)))
        })
        .on_click(move |_, _, cx| app.update(cx, |state, cx| state.toggle_call_chat(cx)))
}

fn hand_button(app: &Entity<AppState>, model: &CallModel) -> Stateful<Div> {
    let raised = model.own_hand.is_some();
    let background = if raised { theme::accent() } else { theme::surface_raised() };
    let button = control_shell("call-hand", background).child(div().text_size(px(CONTROL_ICON)).child(HAND_GLYPH));
    let tooltip = if raised { "Lower hand" } else { "Raise hand" };
    let button = button.tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx));
    if !model.can_react() {
        return button.opacity(DISABLED_OPACITY).cursor_default();
    }
    let app = app.clone();
    button.on_click(move |_, _, cx| {
        cx.stop_propagation();
        app.update(cx, |state, cx| state.toggle_call_hand(cx));
    })
}

fn reactions_button(app: &Entity<AppState>, model: &CallModel) -> AnyElement {
    let face = control_shell("call-reactions-face", theme::surface_raised())
        .child(symbol("mood", CONTROL_ICON, theme::text()));
    if !model.can_react() {
        return face.opacity(DISABLED_OPACITY).cursor_default().into_any_element();
    }
    let app = app.clone();
    Popover::new("call-reactions-popover")
        .anchor(Anchor::BottomLeft)
        .offset(px(10.))
        .trigger(Button::new("call-reactions").ghost().p_0().size(px(CONTROL_SIZE)).rounded_full().child(face))
        .content(move |_, _, cx| {
            let popover = cx.entity().downgrade();
            h_flex().id("call-reactions-row").gap(px(4.)).children(Reaction::ALL.into_iter().map(|reaction| {
                let app = app.clone();
                let popover = popover.clone();
                div()
                    .id(reaction.name())
                    .size(px(REACTION_CELL))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(8.))
                    .text_size(px(24.))
                    .cursor_pointer()
                    .hover(|cell| cell.bg(theme::border_strong()))
                    .child(reaction_glyph(reaction))
                    .on_click(move |_, window, cx| {
                        app.update(cx, |state, cx| state.send_call_reaction(reaction, cx));
                        popover.update(cx, |state, cx| state.dismiss(window, cx)).ok();
                    })
            }))
        })
        .into_any_element()
}

fn leave_menu(app: &Entity<AppState>, model: &CallModel) -> impl IntoElement {
    let leave_app = app.clone();
    let end_app = app.clone();
    let can_end = model.can_end_meeting;
    let lower_app = app.clone();
    let can_lower_all = model.can_lower_hands() && model.any_hand_raised();
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
            let lower_app = lower_app.clone();
            let popup = popup.min_w(px(MENU_WIDTH)).item(
                PopupMenuItem::new("Leave")
                    .icon(IconName::PhoneOff)
                    .on_click(move |_, _, cx| leave_app.update(cx, |state, cx| state.leave_call(cx))),
            );
            let popup = if can_lower_all {
                popup.item(
                    PopupMenuItem::new("Lower all hands")
                        .on_click(move |_, _, cx| lower_app.update(cx, |state, cx| state.lower_all_call_hands(cx))),
                )
            } else {
                popup
            };
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

fn person_tile(app: &Entity<AppState>, state: &AppState, model: &CallModel, remote: &Tile, size: TileSize, now: Instant) -> AnyElement {
    let extras = TileExtras {
        hand: remote.hand.map(|rank| model.hand_position(rank)),
        reaction: model.chip_of(&remote.mri, now).map(|chip| (chip.reaction, chip.serial)),
    };
    let element = person_tile_body(state, model, remote, size, now, &extras);
    if !(model.can_lower_hands() && remote.hand.is_some()) {
        return element;
    }
    let (app, mri) = (app.clone(), remote.mri.clone());
    div()
        .id(SharedString::from(format!("call-tile-{}", remote.mri)))
        .flex_none()
        .child(element)
        .context_menu(move |popup, _, _| {
            let (app, mri) = (app.clone(), mri.clone());
            popup.item(PopupMenuItem::new("Lower hand").on_click(move |_, _, cx| {
                app.update(cx, |state, cx| state.lower_call_hand(mri.clone(), cx));
            }))
        })
        .into_any_element()
}

fn person_tile_body(state: &AppState, model: &CallModel, remote: &Tile, size: TileSize, now: Instant, extras: &TileExtras) -> AnyElement {
    let video = state
        .call
        .as_ref()
        .filter(|_| remote.has_video)
        .and_then(|call| call.pictures.live(&VideoKey::Person(remote.mri.clone()), now));
    if let Some(image) = video {
        return video_tile(size, image, &remote.name, model.tile_speaking(remote), remote.muted, extras).into_any_element();
    }
    let waiting = remote.state == TileState::Invited;
    let avatar = person_avatar(&state.directory, remote.user_id.as_deref(), &remote.name, size.avatar);
    let avatar = if waiting { pulsing(avatar, "call-invited-pulse") } else { avatar };
    let caption = model.tile_caption(remote).map(|text| {
        let label = div().text_size(px(12.)).text_color(theme::text_muted()).child(text);
        if waiting { pulsing(label, "call-invited-caption") } else { label.into_any_element() }
    });
    tile(size, avatar, &remote.name, model.tile_speaking(remote), remote.muted, caption, extras).into_any_element()
}

fn own_tile(state: &AppState, model: &CallModel, size: TileSize, now: Instant) -> AnyElement {
    let extras = TileExtras {
        hand: model.own_hand.map(|rank| model.hand_position(rank)),
        reaction: model.own_chip(now).map(|chip| (chip.reaction, chip.serial)),
    };
    let me = state.directory.me.as_ref();
    let self_view = state
        .call
        .as_ref()
        .and_then(|call| call.pictures.live(&VideoKey::LocalCamera, now));
    if let Some(image) = self_view {
        return video_tile(size, image, "You", model.local_speaking, model.muted, &extras).into_any_element();
    }
    let avatar = person_avatar(
        &state.directory,
        me.map(|person| person.user_id.as_str()),
        me.map_or("You", |person| person.display_name.as_str()),
        size.avatar,
    );
    tile(size, avatar, "You", model.local_speaking, model.muted, None, &extras).into_any_element()
}

fn remote_tiles(app: &Entity<AppState>, state: &AppState, model: &CallModel, size: TileSize, hidden_cap: GridLayout, now: Instant) -> Vec<AnyElement> {
    let visible = model.visible_tiles();
    let mut cells: Vec<AnyElement> = visible
        .iter()
        .take(hidden_cap.shown_others)
        .map(|remote| person_tile(app, state, model, remote, size, now))
        .collect();
    if hidden_cap.hidden > 0 {
        cells.push(overflow_tile(size, hidden_cap.hidden).into_any_element());
    }
    cells
}

fn people_strip(app: &Entity<AppState>, state: &AppState, model: &CallModel, now: Instant) -> Stateful<Div> {
    let mut cells: Vec<AnyElement> = model
        .strip_tiles()
        .into_iter()
        .map(|remote| person_tile(app, state, model, remote, STRIP_TILE, now))
        .collect();
    let own_index = model.own_cell_index(cells.len());
    cells.insert(own_index, own_tile(state, model, STRIP_TILE, now));
    v_flex()
        .id("call-strip")
        .flex_none()
        .w(px(STRIP_TILE.width))
        .max_h(px(STRIP_VISIBLE * (STRIP_TILE.height + STRIP_GAP) - STRIP_GAP))
        .gap(px(STRIP_GAP))
        .overflow_y_scroll()
        .children(cells)
}

pub fn render_call_view(app: &Entity<AppState>, state: &AppState, chat: Option<Entity<ConversationView>>) -> Option<AnyElement> {
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

    let max_tiles = if chat.is_some() { CHAT_OPEN_TILES } else { MAX_TILES };
    let layout = grid_layout(model.visible_tiles().len(), max_tiles);
    let cells_for_size = if chat.is_some() { MAX_TILES } else { layout.shown_others + usize::from(layout.hidden > 0) + 1 };
    let size = tile_size(cells_for_size);
    let mut cells = remote_tiles(app, state, model, size, layout, now);
    let own_index = model.own_cell_index(cells.len());
    cells.insert(own_index, own_tile(state, model, size, now));
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
        .child(share_button(app, model, state.call_share_sound))
        .child(hand_button(app, model))
        .child(reactions_button(app, model))
        .child(devices_button(app, model))
        .when(call.chat_thread().is_some(), |row| row.child(chat_button(app, call.chat_open, state.call_chat_unread())))
        .child(leave_button(app, model, "call-leave", false))
        .when(model.kind == CallKind::Meeting, |row| row.child(leave_menu(app, model)));

    let column = v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(theme::background())
            .child(header)
            .children(sharing_banner(app, model, state.call_share_sound))
            .child(match stage {
                Some(stage) => h_flex()
                    .flex_1()
                    .min_h_0()
                    .p(px(GRID_PADDING))
                    .gap(px(TILE_GAP))
                    .items_center()
                    .child(stage)
                    .child(people_strip(app, state, model, now))
                    .into_any_element(),
                None => h_flex()
                    .id("call-grid")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex_wrap()
                    .content_center()
                    .p(px(GRID_PADDING))
                    .gap(px(TILE_GAP))
                    .items_center()
                    .justify_center()
                    .children(cells)
                    .into_any_element(),
            })
            .child(controls);
    Some(
        h_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(column)
            .children(chat.map(|chat| chat_panel(app, chat)))
            .into_any_element(),
    )
}

fn chat_panel(app: &Entity<AppState>, chat: Entity<ConversationView>) -> AnyElement {
    let app = app.clone();
    v_flex()
        .id("call-chat-panel")
        .w(px(CHAT_PANEL_WIDTH))
        .h_full()
        .flex_none()
        .border_l_1()
        .border_color(theme::border())
        .child(
            h_flex()
                .w_full()
                .h(px(CHAT_HEADER_HEIGHT))
                .flex_none()
                .px(px(16.))
                .items_center()
                .justify_between()
                .border_b_1()
                .border_color(theme::border())
                .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child("Chat"))
                .child(
                    div()
                        .id("call-chat-close")
                        .size(px(28.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .cursor_pointer()
                        .hover(|button| button.bg(theme::row_hover()))
                        .tooltip(|window, cx| Tooltip::new("Close the chat").build(window, cx))
                        .child(icon(IconName::Close, 16., theme::text_muted()))
                        .on_click(move |_, _, cx| app.update(cx, |state, cx| state.toggle_call_chat(cx))),
                ),
        )
        .child(chat)
        .into_any_element()
}
