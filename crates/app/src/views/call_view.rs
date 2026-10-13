use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use calling::{DeviceEntry, MicSettings, NoiseSuppression, Reaction, VideoKey};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    popover::Popover,
    switch::Switch,
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::avatar::person_avatar;
use super::call_control_fold::{
    CONTROL_GAP, CONTROLS_PADDING, Foldable, FoldedControls, fixed_controls_width, folded_for_width,
};
use super::call_extras::{
    CameraMenu, consent_overlay, consult_banner, hold_panel, recording_chip, return_button,
    room_chip, whiteboard_stage,
};
use super::call_health::{health_banner, health_indicator};
use super::call_layouts::{measured_stage, stage_region};
use super::call_organizer::{
    camera_menu_button, captions_hint_chip, captions_overlay, chevron_button, lobby_banner,
    more_button, pin_chip, spotlight_chip, with_tile_menu,
};
use super::call_people::{lock_banner, lock_chip, options_dialog, people_button, people_panel};
use super::call_stage::{StageMode, popped_placeholder, render_stage};
use super::call_view_menu::view_button;
use super::conversation::ConversationView;
use super::shell::ISLAND_GAP;
use super::widgets::{Corner, corner_badge, count_badge, icon};
use crate::app_state::AppState;
use crate::call::REACTION_SHOWN;
use crate::call::TileSize;
use crate::call::{ActiveCall, CallKind, CallModel, Tile, TileState};
use crate::rows::reaction_glyph as chat_reaction_glyph;
use crate::theme;

const HEADER_HEIGHT: f32 = 60.;
const RING_WIDTH: f32 = 3.;
const RING_GAP: f32 = 4.;
pub(super) const CONTROL_SIZE: f32 = 44.;
const CONTROL_ICON: f32 = 20.;
pub(super) const MENU_WIDTH: f32 = 280.;
const PULSE_PERIOD: Duration = Duration::from_millis(1400);
const BLINK_HALF_PERIOD: Duration = Duration::from_millis(500);
const BLINK_DIMMED_OPACITY: f32 = 0.45;
pub(super) const DISABLED_OPACITY: f32 = 0.4;
pub(super) const TILE_GAP: f32 = 20.;
pub(super) const GRID_PADDING: f32 = 24.;
pub(super) const STRIP_GAP: f32 = 12.;
pub(super) const STRIP_VISIBLE: f32 = 4.;
const TILE_RADIUS: f32 = 12.;
const NAME_PLATE_BACKGROUND: f32 = 0.6;
pub const NO_MICROPHONE_HINT: &str = "No microphone found";
const AUDIO_OFF_HINT: &str = "Audio is off for this meeting";
pub const NO_CAMERA_HINT: &str = "No camera found";
pub const NO_SHARE_HINT: &str = "Sharing starts once the call is connected";
pub const NO_WINDOWS_HINT: &str = "No windows found";
pub(super) const BANNER_HEIGHT: f32 = 40.;
const MAX_WINDOW_ITEMS: usize = 12;
pub const LOBBY_TEXT: &str = "Waiting in the lobby...";
pub(super) const CHAT_PANEL_WIDTH: f32 = 320.;
const OVERLAY_BELOW: f32 = 720.;
const CONTROLS_HEIGHT: f32 = 2. * 16. + CONTROL_SIZE;
const CHAT_HEADER_HEIGHT: f32 = 48.;
const REACTION_RISE: f32 = 24.;
const REACTION_FADE_FROM: f32 = 0.75;
const REACTION_CELL: f32 = 40.;
const SHARE_SOUND_LABEL: &str = "Include computer sound";
const SHARE_MOTION_LABEL: &str = "Optimize for motion";
const SHARE_STANDOUT_LABEL: &str = "Show me over my content";
const SHARE_STANDOUT_NEEDS_CAMERA: &str = "Turn your camera on first";
const NOISE_SUPPRESSION_LABEL: &str = "Noise suppression";
const MUSIC_MODE_LABEL: &str = "Music mode";
const PUSH_TO_TALK_HINT: &str = "Unmuted - release Ctrl+Space to mute";
const PUSH_TO_TALK_TOP: f32 = 12.;

pub fn reaction_glyph(reaction: Reaction) -> String {
    match reaction {
        Reaction::Applause => "\u{1F44F}".to_owned(),
        other => chat_reaction_glyph(other.name()),
    }
}

#[derive(Default)]
struct TileExtras {
    hand: Option<usize>,
    own: bool,
    reaction: Option<(Reaction, u64)>,
    spotlight: bool,
    pinned: bool,
}

fn hand_badge(position: usize, own: bool) -> Div {
    h_flex()
        .h(px(24.))
        .px(px(8.))
        .gap(px(4.))
        .items_center()
        .rounded_full()
        .bg(theme::amber())
        .text_color(hsla(0., 0., 0.08, 1.))
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(if own {
            "Hand raised".to_owned()
        } else {
            format!("Hand {position}")
        })
}

fn reaction_chip(reaction: Reaction, serial: u64) -> AnyElement {
    div()
        .absolute()
        .right(px(12.))
        .bottom(px(32.))
        .size(px(36.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(theme::popup())
        .text_size(px(20.))
        .child(reaction_glyph(reaction))
        .with_animation(
            ("call-reaction", serial as usize),
            Animation::new(REACTION_SHOWN),
            |chip, delta| {
                let fade = ((delta - REACTION_FADE_FROM) / (1. - REACTION_FADE_FROM)).clamp(0., 1.);
                chip.bottom(px(32. + REACTION_RISE * delta))
                    .opacity(1. - fade)
            },
        )
        .into_any_element()
}

fn tile_overlays(extras: &TileExtras) -> Vec<AnyElement> {
    let badges = (extras.hand.is_some() || extras.spotlight || extras.pinned).then(|| {
        h_flex()
            .absolute()
            .top(px(8.))
            .left(px(8.))
            .gap(px(6.))
            .children(extras.hand.map(|position| hand_badge(position, extras.own)))
            .when(extras.spotlight, |row| row.child(spotlight_chip()))
            .when(extras.pinned, |row| row.child(pin_chip()))
            .into_any_element()
    });
    let reaction = extras
        .reaction
        .map(|(reaction, serial)| reaction_chip(reaction, serial));
    badges.into_iter().chain(reaction).collect()
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

fn blinking(element: impl IntoElement + 'static) -> AnyElement {
    let dimmed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .is_ok_and(|elapsed| (elapsed.as_millis() / BLINK_HALF_PERIOD.as_millis()) % 2 == 1);
    div()
        .child(element)
        .opacity(if dimmed { BLINK_DIMMED_OPACITY } else { 1. })
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
        .top(px(8.))
        .right(px(8.))
        .size(px(24.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(theme::red())
        .child(icon(IconName::MicOff, 13., theme::white()))
}

fn video_tile(
    size: TileSize,
    image: Arc<RenderImage>,
    name: &str,
    speaking: bool,
    muted: bool,
    extras: &TileExtras,
) -> Div {
    div()
        .relative()
        .w(px(size.width))
        .h(px(size.height))
        .flex_none()
        .overflow_hidden()
        .rounded(px(TILE_RADIUS))
        .bg(theme::row_hover())
        .border_2()
        .border_color(if speaking {
            theme::green()
        } else {
            transparent_black()
        })
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
                .py(px(4.))
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
        .top(px(8.))
        .right(px(8.))
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
        .bg(theme::row_hover())
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

pub(super) fn round_control(
    id: &'static str,
    glyph: IconName,
    background: Hsla,
    foreground: Hsla,
) -> Stateful<Div> {
    control_shell(id, background).child(icon(glyph, CONTROL_ICON, foreground))
}

pub fn mute_button(
    app: &Entity<AppState>,
    model: &CallModel,
    id: &'static str,
    size: f32,
) -> Stateful<Div> {
    let (glyph, background, foreground) = if model.muted {
        (IconName::MicOff, theme::red(), theme::white())
    } else {
        (IconName::Mic, theme::surface_raised(), theme::text())
    };
    let button = round_control(id, glyph, background, foreground).size(px(size));
    if !model.can_unmute() {
        let hint = if model.audio_off {
            AUDIO_OFF_HINT
        } else {
            NO_MICROPHONE_HINT
        };
        return button
            .opacity(DISABLED_OPACITY)
            .cursor_default()
            .tooltip(move |window, cx| Tooltip::new(hint).build(window, cx));
    }
    let app = app.clone();
    button.on_click(move |_, _, cx| {
        cx.stop_propagation();
        app.update(cx, |state, cx| state.toggle_call_mute(cx));
    })
}

pub fn leave_button(
    app: &Entity<AppState>,
    model: &CallModel,
    id: &'static str,
    compact: bool,
) -> Stateful<Div> {
    let label = if model.is_connecting() {
        "Cancel"
    } else {
        "Leave"
    };
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
        .child(icon(
            IconName::PhoneOff,
            if compact { 14. } else { 18. },
            theme::white(),
        ))
        .child(label)
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            app.update(cx, |state, cx| state.leave_call(cx));
        })
}

fn share_stage_element(app: &Entity<AppState>, state: &AppState, call: &ActiveCall) -> Option<AnyElement> {
    if call.share_stage.popped && call.model.screen_sharer.is_some() {
        return Some(popped_placeholder(app));
    }
    render_stage(app, state, StageMode::Docked).or_else(|| whiteboard_stage(app, &call.model))
}

pub(super) fn camera_button(app: &Entity<AppState>, model: &CallModel) -> Stateful<Div> {
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

fn motion_switch_item(app: &Entity<AppState>, motion: bool) -> PopupMenuItem {
    let app = app.clone();
    PopupMenuItem::element(move |_, _| {
        h_flex()
            .id("call-share-motion-row")
            .w_full()
            .gap(px(16.))
            .items_center()
            .justify_between()
            .child(SHARE_MOTION_LABEL)
            .child(Switch::new("call-share-motion").checked(motion))
    })
    .on_click(move |_, _, cx| app.update(cx, |state, cx| state.set_call_share_motion(!motion, cx)))
}

fn standout_switch_item(app: &Entity<AppState>, standout: bool, camera_on: bool) -> PopupMenuItem {
    let app = app.clone();
    PopupMenuItem::element(move |_, _| {
        v_flex()
            .id("call-share-standout-row")
            .w_full()
            .when(!camera_on, |row| row.opacity(DISABLED_OPACITY))
            .child(
                h_flex()
                    .w_full()
                    .gap(px(16.))
                    .items_center()
                    .justify_between()
                    .child(SHARE_STANDOUT_LABEL)
                    .child(Switch::new("call-share-standout").checked(standout && camera_on)),
            )
            .when(!camera_on, |row| {
                row.child(
                    div()
                        .text_size(px(11.))
                        .text_color(theme::text_muted())
                        .child(SHARE_STANDOUT_NEEDS_CAMERA),
                )
            })
    })
    .disabled(!camera_on)
    .on_click(move |_, _, cx| app.update(cx, |state, cx| state.set_call_share_standout(!standout, cx)))
}

fn music_mode_item(app: &Entity<AppState>, mic: MicSettings) -> PopupMenuItem {
    let app = app.clone();
    PopupMenuItem::element(move |_, _| {
        h_flex()
            .id("call-music-mode-row")
            .w_full()
            .gap(px(16.))
            .items_center()
            .justify_between()
            .child(MUSIC_MODE_LABEL)
            .child(Switch::new("call-music-mode").checked(mic.music_mode))
    })
    .on_click(move |_, _, cx| {
        app.update(cx, |state, cx| {
            let settings = MicSettings {
                music_mode: !mic.music_mode,
                ..state.call_mic
            };
            state.set_call_mic(settings, cx)
        })
    })
}

fn push_to_talk_chip(call: &ActiveCall) -> Option<AnyElement> {
    call.push_to_talk.is_held().then(|| {
        div()
            .id("call-push-to-talk")
            .absolute()
            .top(px(HEADER_HEIGHT + PUSH_TO_TALK_TOP))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(
                div()
                    .debug_selector(|| "call/push-to-talk".into())
                    .px(px(12.))
                    .py(px(6.))
                    .rounded_full()
                    .bg(hsla(0., 0., 0., NAME_PLATE_BACKGROUND))
                    .text_color(theme::white())
                    .text_size(px(13.))
                    .child(PUSH_TO_TALK_HINT),
            )
            .into_any_element()
    })
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ShareOptions {
    pub sound: bool,
    pub motion: bool,
    pub standout: bool,
}

impl ShareOptions {
    pub(super) fn of(state: &AppState) -> Self {
        ShareOptions {
            sound: state.call_share_sound,
            motion: state.call_share_motion,
            standout: state.call_share_standout,
        }
    }
}

fn share_items(
    popup: PopupMenu,
    app: &Entity<AppState>,
    model: &CallModel,
    options: ShareOptions,
) -> PopupMenu {
    let screens = model.screens();
    let single_screen = screens.len() == 1;
    let mut popup = popup
        .min_w(px(MENU_WIDTH))
        .item(sound_switch_item(app, options.sound))
        .item(motion_switch_item(app, options.motion))
        .item(standout_switch_item(app, options.standout, model.camera_on))
        .separator();
    for (index, source) in screens.into_iter().enumerate() {
        let label = if single_screen {
            "Share entire screen".to_owned()
        } else {
            format!("Share screen {}", index + 1)
        };
        let source = source.clone();
        let app = app.clone();
        popup = popup.item(PopupMenuItem::new(label).icon(IconName::Monitor).on_click(
            move |_, _, cx| app.update(cx, |state, cx| state.start_call_share(source.clone(), cx)),
        ));
    }
    popup = popup
        .separator()
        .item(PopupMenuItem::label("Share a window..."));
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
                .on_click(move |_, _, cx| {
                    app.update(cx, |state, cx| state.start_call_share(source.clone(), cx))
                }),
        );
    }
    popup
}

fn share_button(
    app: &Entity<AppState>,
    model: &CallModel,
    options: ShareOptions,
    menu_open: bool,
) -> AnyElement {
    if model.local_share.is_some() {
        let app = app.clone();
        return round_control(
            "call-share-stop",
            IconName::ScreenShareOff,
            theme::accent(),
            theme::on_accent(),
        )
        .tooltip(|window, cx| Tooltip::new("Stop sharing").build(window, cx))
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            app.update(cx, |state, cx| state.stop_call_share(cx));
        })
        .into_any_element();
    }
    let face = round_control(
        "call-share-face",
        IconName::ScreenShare,
        theme::surface_raised(),
        theme::text(),
    );
    if !model.can_share() {
        return face
            .opacity(DISABLED_OPACITY)
            .cursor_default()
            .tooltip(|window, cx| Tooltip::new(NO_SHARE_HINT).build(window, cx))
            .into_any_element();
    }
    let app = app.clone();
    let open_app = app.clone();
    let model = model.clone();
    Button::new("call-share")
        .ghost()
        .p_0()
        .size(px(CONTROL_SIZE))
        .rounded_full()
        .child(face)
        .dropdown_menu_with_anchor(Anchor::BottomLeft, move |popup, _, _| {
            share_items(popup, &app, &model, options)
        })
        .open(menu_open)
        .on_open_change(move |open, _, cx| {
            open_app.update(cx, |state, cx| state.set_call_share_menu(*open, cx))
        })
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
                    .bg(theme::on_accent().opacity(0.2))
                    .cursor_pointer()
                    .hover(|button| button.opacity(0.9))
                    .child(if sound {
                        "Computer sound on"
                    } else {
                        "Computer sound off"
                    })
                    .on_click(move |_, _, cx| {
                        sound_app.update(cx, |state, cx| state.set_call_share_sound(!sound, cx))
                    }),
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
                    .on_click(move |_, _, cx| {
                        app.update(cx, |state, cx| state.stop_call_share(cx))
                    }),
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
        .tooltip(move |window, cx| {
            Tooltip::new(if open {
                "Close the chat"
            } else {
                "Open the chat"
            })
            .build(window, cx)
        })
        .when(unread > 0, |button| {
            button.child(corner_badge(Corner::TopRight, 4.).child(count_badge(unread, false)))
        })
        .on_click(move |_, _, cx| app.update(cx, |state, cx| state.toggle_call_chat(cx)))
}

fn hand_button(app: &Entity<AppState>, model: &CallModel) -> Stateful<Div> {
    let raised = model.own_hand.is_some();
    let background = if raised {
        theme::accent()
    } else {
        theme::surface_raised()
    };
    let foreground = if raised {
        theme::on_accent()
    } else {
        theme::text()
    };
    let button = control_shell("call-hand", background).child(icon(
        IconName::Hand,
        CONTROL_ICON,
        foreground,
    ));
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
    let face = control_shell("call-reactions-face", theme::surface_raised()).child(icon(
        IconName::FaceSlightlySmiling,
        CONTROL_ICON,
        theme::text(),
    ));
    if !model.can_react() {
        return face
            .opacity(DISABLED_OPACITY)
            .cursor_default()
            .into_any_element();
    }
    let app = app.clone();
    Popover::new("call-reactions-popover")
        .anchor(Anchor::BottomLeft)
        .offset(px(10.))
        .trigger(
            Button::new("call-reactions")
                .ghost()
                .p_0()
                .size(px(CONTROL_SIZE))
                .rounded_full()
                .child(face),
        )
        .content(move |_, _, cx| {
            let popover = cx.entity().downgrade();
            h_flex()
                .id("call-reactions-row")
                .gap(px(4.))
                .children(Reaction::ALL.into_iter().map(|reaction| {
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
                            popover
                                .update(cx, |state, cx| state.dismiss(window, cx))
                                .ok();
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
                    .on_click(move |_, _, cx| {
                        leave_app.update(cx, |state, cx| state.leave_call(cx))
                    }),
            );
            let popup = if can_lower_all {
                popup.item(
                    PopupMenuItem::new("Lower all hands").on_click(move |_, _, cx| {
                        lower_app.update(cx, |state, cx| state.lower_all_call_hands(cx))
                    }),
                )
            } else {
                popup
            };
            if can_end {
                popup.item(
                    PopupMenuItem::new("End meeting for everyone")
                        .icon(IconName::PhoneOff)
                        .on_click(move |_, _, cx| {
                            end_app.update(cx, |state, cx| state.end_meeting_for_all(cx))
                        }),
                )
            } else {
                popup
            }
        })
}

pub(super) fn device_items(
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

fn devices_menu(
    popup: PopupMenu,
    app: &Entity<AppState>,
    model: &CallModel,
    mic: MicSettings,
) -> PopupMenu {
    if model.audio_off {
        return popup.item(PopupMenuItem::new(AUDIO_OFF_HINT).disabled(true));
    }
    let input_app = app.clone();
    let output_app = app.clone();
    let inputs = device_items(
        model.devices.input_entries(),
        &model.input,
        move |choice, cx| {
            input_app.update(cx, |state, cx| state.select_call_input(choice, cx));
        },
    );
    let outputs = device_items(
        model.devices.output_entries(),
        &model.output,
        move |choice, cx| {
            output_app.update(cx, |state, cx| state.select_call_output(choice, cx));
        },
    );
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
    popup = popup
        .separator()
        .item(PopupMenuItem::label(NOISE_SUPPRESSION_LABEL));
    for level in NoiseSuppression::ALL {
        let level_app = app.clone();
        popup = popup.item(
            PopupMenuItem::new(level.label())
                .checked(level == mic.noise_suppression)
                .disabled(mic.music_mode)
                .on_click(move |_, _, cx| {
                    level_app.update(cx, |state, cx| {
                        let settings = MicSettings {
                            noise_suppression: level,
                            ..state.call_mic
                        };
                        state.set_call_mic(settings, cx)
                    })
                }),
        );
    }
    popup = popup
        .item(music_mode_item(app, mic))
        .separator()
        .item(PopupMenuItem::label("Speaker"));
    for item in outputs {
        popup = popup.item(item);
    }
    popup
}

fn mic_menu_button(app: &Entity<AppState>, model: &CallModel, mic: MicSettings) -> impl IntoElement {
    let app = app.clone();
    let model = model.clone();
    chevron_button("call-mic-menu", "call-mic-menu-face")
        .dropdown_menu_with_anchor(Anchor::BottomLeft, move |popup, _, _| {
            devices_menu(popup, &app, &model, mic)
        })
}

pub(super) fn person_tile(
    app: &Entity<AppState>,
    state: &AppState,
    model: &CallModel,
    remote: &Tile,
    size: TileSize,
    now: Instant,
) -> AnyElement {
    let extras = TileExtras {
        hand: remote.hand.map(|rank| model.hand_position(rank)),
        own: false,
        reaction: model
            .chip_of(&remote.mri, now)
            .map(|chip| (chip.reaction, chip.serial)),
        spotlight: remote.spotlight.is_some(),
        pinned: model.is_pinned(&remote.mri),
    };
    let element = person_tile_body(state, model, remote, size, now, &extras);
    with_tile_menu(app, model, &remote.mri, &remote.name, false, element)
}

fn person_tile_body(
    state: &AppState,
    model: &CallModel,
    remote: &Tile,
    size: TileSize,
    now: Instant,
    extras: &TileExtras,
) -> AnyElement {
    let video = state
        .call
        .as_ref()
        .filter(|_| remote.has_video || model.is_one_to_one())
        .and_then(|call| {
            call.pictures
                .live(&VideoKey::Person(remote.mri.clone()), now)
        });
    if let Some(image) = video {
        return video_tile(
            size,
            image,
            &remote.name,
            model.tile_speaking(remote),
            remote.muted,
            extras,
        )
        .into_any_element();
    }
    let waiting = remote.state == TileState::Invited;
    let avatar = person_avatar(
        &state.directory,
        remote.user_id.as_deref(),
        &remote.name,
        size.avatar,
    );
    let caption = model.tile_caption(remote).map(|text| {
        let label = div()
            .text_size(px(12.))
            .text_color(theme::text_muted())
            .child(text);
        if waiting {
            blinking(label)
        } else {
            label.into_any_element()
        }
    });
    tile(
        size,
        avatar,
        &remote.name,
        model.tile_speaking(remote),
        remote.muted,
        caption,
        extras,
    )
    .into_any_element()
}

pub(super) fn own_tile(
    app: &Entity<AppState>,
    state: &AppState,
    model: &CallModel,
    size: TileSize,
    now: Instant,
) -> AnyElement {
    let own_mri = model.own_mri.clone().unwrap_or_default();
    let extras = TileExtras {
        hand: model.own_hand.map(|rank| model.hand_position(rank)),
        own: true,
        reaction: model.own_chip(now).map(|chip| (chip.reaction, chip.serial)),
        spotlight: model.own_spotlight.is_some(),
        pinned: model.is_pinned(&own_mri),
    };
    let element = own_tile_body(state, model, size, now, &extras);
    if own_mri.is_empty() {
        return element;
    }
    with_tile_menu(app, model, &own_mri, "You", true, element)
}

fn own_tile_body(
    state: &AppState,
    model: &CallModel,
    size: TileSize,
    now: Instant,
    extras: &TileExtras,
) -> AnyElement {
    let me = state.directory.me.as_ref();
    let self_view = state
        .call
        .as_ref()
        .and_then(|call| call.pictures.live(&VideoKey::LocalCamera, now));
    if let Some(image) = self_view {
        return video_tile(
            size,
            image,
            "You",
            model.local_speaking,
            model.muted,
            extras,
        )
        .into_any_element();
    }
    let avatar = person_avatar(
        &state.directory,
        me.map(|person| person.user_id.as_str()),
        me.map_or("You", |person| person.display_name.as_str()),
        size.avatar,
    );
    tile(
        size,
        avatar,
        "You",
        model.local_speaking,
        model.muted,
        None,
        extras,
    )
    .into_any_element()
}

fn folded_controls(state: &AppState, call: &ActiveCall, fullscreen: bool) -> FoldedControls {
    let model = &call.model;
    let has_more = model.kind == CallKind::Meeting || model.is_one_to_one();
    if !has_more {
        return FoldedControls::default();
    }
    let mut present = vec![Foldable::View];
    if model.has_reaction_controls() {
        present.extend([Foldable::Reactions, Foldable::RaiseHand]);
    }
    if model.kind != CallKind::Test {
        present.push(Foldable::People);
    }
    if call.chat_thread().is_some() {
        present.push(Foldable::Chat);
    }
    let fixed = fixed_controls_width(
        model.can_use_camera(),
        has_more,
        model.kind == CallKind::Meeting,
    );
    FoldedControls {
        items: folded_for_width(measured_stage(call).0, fixed, &present),
        fullscreen,
        people_open: call.people_open,
        chat_open: call.chat_open,
        chat_unread: state.call_chat_unread(),
    }
}

pub fn render_call_view(
    app: &Entity<AppState>,
    state: &AppState,
    chat: Option<Entity<ConversationView>>,
    fullscreen: bool,
    island_width: f32,
) -> Option<AnyElement> {
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
        .child(icon(IconName::Phone, 18., theme::accent_text()))
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(px(16.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(model.title.clone()),
        )
        .child(
            div()
                .flex_none()
                .text_size(px(13.))
                .text_color(status_color)
                .child(status),
        )
        .children(health_indicator(model))
        .children(recording_chip(model))
        .children(lock_chip(model))
        .children(room_chip(model))
        .child(div().flex_1())
        .children(return_button(app, model));

    let hold = hold_panel(app, model);
    let share_stage = share_stage_element(app, state, call);
    let region = stage_region(app, state, call, now, hold, share_stage);

    let folded = folded_controls(state, call, fullscreen);
    let controls = h_flex()
        .w_full()
        .flex_none()
        .py(px(16.))
        .px(px(CONTROLS_PADDING))
        .gap(px(CONTROL_GAP))
        .items_center()
        .justify_center()
        .child(
            h_flex()
                .debug_selector(|| "shell/islands/main/call-controls/mic".into())
                .h(px(CONTROL_SIZE))
                .items_center()
                .rounded_full()
                .bg(theme::surface_raised())
                .child(mute_button(app, model, "call-mute", CONTROL_SIZE))
                .child(mic_menu_button(app, model, state.call_mic)),
        )
        .child(
            h_flex()
                .h(px(CONTROL_SIZE))
                .items_center()
                .rounded_full()
                .bg(theme::surface_raised())
                .child(camera_button(app, model))
                .children(camera_menu_button(app, model, CameraMenu::of(state))),
        )
        .child(share_button(
            app,
            model,
            ShareOptions::of(state),
            call.share_menu_open,
        ))
        .when(
            model.has_reaction_controls() && !folded.folds(Foldable::RaiseHand),
            |row| row.child(hand_button(app, model)),
        )
        .when(
            model.has_reaction_controls() && !folded.folds(Foldable::Reactions),
            |row| row.child(reactions_button(app, model)),
        )
        .when(!folded.folds(Foldable::View), |row| {
            row.child(view_button(app, model, fullscreen))
        })
        .children(more_button(app, model, &folded))
        .when(
            model.kind != CallKind::Test && !folded.folds(Foldable::People),
            |row| row.child(people_button(app, call.people_open, model.raised_hands())),
        )
        .when(
            call.chat_thread().is_some() && !folded.folds(Foldable::Chat),
            |row| row.child(chat_button(app, call.chat_open, state.call_chat_unread())),
        )
        .child(
            leave_button(app, model, "call-leave", false)
                .debug_selector(|| "shell/islands/main/call-controls/leave".into()),
        )
        .when(model.kind == CallKind::Meeting, |row| {
            row.child(leave_menu(app, model))
        });

    let top = v_flex()
        .w_full()
        .flex_none()
        .child(header)
        .children(lobby_banner(app, model, call.lobby_list_open))
        .children(lock_banner(app, model))
        .children(health_banner(app, model))
        .children(sharing_banner(app, model, state.call_share_sound))
        .children(consult_banner(app, state));

    let chrome_visible = call
        .fullscreen
        .is_none_or(|fullscreen| fullscreen.chrome_visible(now));
    let move_app = app.clone();
    let base = v_flex()
        .relative()
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_hidden()
        .bg(theme::surface());
    let column = if fullscreen {
        base.on_mouse_move(move |_, _, cx| {
            move_app.update(cx, |state, cx| state.touch_call_chrome(cx))
        })
        .when(chrome_visible, |column| {
            column.child(fullscreen_chrome("call-chrome-top", app, top))
        })
        .child(region)
        .when(chrome_visible, |column| {
            column.child(fullscreen_chrome("call-chrome-bottom", app, controls))
        })
    } else {
        base.rounded(px(10.))
            .child(top.debug_selector(|| "shell/islands/main/call-header".into()))
            .child(region)
            .child(controls.debug_selector(|| "shell/islands/main/call-controls".into()))
    }
    .children(captions_overlay(model, now))
    .children(captions_hint_chip(model))
    .children(push_to_talk_chip(call))
    .children(state.transfer_picker.clone())
    .children(options_dialog(app, call))
    .children(consent_overlay(app, model));
    let panels: Vec<AnyElement> = chat
        .map(|chat| chat_panel(app, chat))
        .into_iter()
        .chain(
            call.transcript_open
                .then(|| state.transcript_panel.clone())
                .flatten()
                .map(IntoElement::into_any_element),
        )
        .chain(people_panel(app, state, call))
        .collect();
    Some(
        h_flex()
            .relative()
            .when(!fullscreen, |row| {
                row.debug_selector(|| "shell/islands/main".into())
            })
            .flex_1()
            .min_w_0()
            .h_full()
            .gap(px(ISLAND_GAP))
            .child(column)
            .children(side_panels(panels, island_width))
            .into_any_element(),
    )
}

fn side_panels(panels: Vec<AnyElement>, island_width: f32) -> Vec<AnyElement> {
    if panels.is_empty() {
        return panels;
    }
    let panels_width = (CHAT_PANEL_WIDTH + ISLAND_GAP) * panels.len() as f32;
    if island_width - panels_width >= OVERLAY_BELOW {
        return panels;
    }
    vec![
        h_flex()
            .absolute()
            .top_0()
            .right_0()
            .bottom(px(CONTROLS_HEIGHT))
            .gap(px(ISLAND_GAP))
            .w(px(panels_width.min(island_width)))
            .children(panels)
            .into_any_element(),
    ]
}

fn fullscreen_chrome(id: &'static str, app: &Entity<AppState>, content: Div) -> Stateful<Div> {
    let hover_app = app.clone();
    div()
        .id(id)
        .w_full()
        .flex_none()
        .on_hover(move |hovered, _, cx| {
            hover_app.update(cx, |state, cx| state.set_call_chrome_hovered(*hovered, cx))
        })
        .child(content)
}

fn chat_panel(app: &Entity<AppState>, chat: Entity<ConversationView>) -> AnyElement {
    let app = app.clone();
    side_panel(
        "call-chat-panel",
        "Chat",
        "Close the chat",
        move |cx| app.update(cx, |state, cx| state.toggle_call_chat(cx)),
        chat,
    )
    .into_any_element()
}

pub fn side_panel(
    panel_id: &'static str,
    title: &'static str,
    close_tooltip: &'static str,
    on_close: impl Fn(&mut App) + 'static,
    body: impl IntoElement,
) -> Stateful<Div> {
    v_flex()
        .id(panel_id)
        .w(px(CHAT_PANEL_WIDTH))
        .h_full()
        .flex_shrink(1.)
        .min_w_0()
        .rounded(px(10.))
        .overflow_hidden()
        .bg(theme::surface())
        .child(
            h_flex()
                .w_full()
                .h(px(CHAT_HEADER_HEIGHT))
                .flex_none()
                .pl(px(20.))
                .pr(px(12.))
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(15.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title),
                )
                .child(
                    div()
                        .id(ElementId::Name(format!("{panel_id}-close").into()))
                        .size(px(28.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(7.))
                        .cursor_pointer()
                        .hover(|button| button.bg(theme::row_hover()))
                        .tooltip(move |window, cx| Tooltip::new(close_tooltip).build(window, cx))
                        .child(icon(IconName::Close, 14., theme::text_muted()))
                        .on_click(move |_, _, cx| on_close(cx)),
                ),
        )
        .child(body)
}
