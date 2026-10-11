use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, tooltip::Tooltip};
use gpui_kit::*;

use super::call_view::{camera_button, mute_button, round_control};
use crate::app_state::AppState;
use crate::call::CallWindows;
use crate::theme;

const BUTTON_SIZE: f32 = 26.;
const BAR_RADIUS: f32 = 10.;
const HANDLE_RADIUS: f32 = 3.;
const STOP_HEIGHT: f32 = 26.;

pub struct ShareBarView {
    windows: Entity<CallWindows>,
    app: Entity<AppState>,
    _windows_subscription: Subscription,
    _app_subscription: Subscription,
}

impl ShareBarView {
    pub fn new(windows: Entity<CallWindows>, cx: &mut Context<Self>) -> Self {
        let app = windows.read(cx).app().clone();
        let windows_subscription = cx.observe(&windows, |_, _, cx| cx.notify());
        let app_subscription = cx.observe(&app, |_, _, cx| cx.notify());
        ShareBarView {
            windows,
            app,
            _windows_subscription: windows_subscription,
            _app_subscription: app_subscription,
        }
    }
}

fn stop_button(app: &Entity<AppState>) -> Stateful<Div> {
    let app = app.clone();
    h_flex()
        .id("share-bar-stop")
        .h(px(STOP_HEIGHT))
        .px(px(10.))
        .flex_none()
        .items_center()
        .rounded_full()
        .bg(theme::red())
        .text_color(theme::white())
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .hover(|button| button.opacity(0.85))
        .child("Stop sharing")
        .on_click(move |_, _, cx| app.update(cx, |state, cx| state.stop_call_share(cx)))
}

fn sound_button(app: &Entity<AppState>, sound: bool) -> Stateful<Div> {
    let app = app.clone();
    let (glyph, background, foreground) = if sound {
        (IconName::Volume2, theme::accent(), theme::on_accent())
    } else {
        (IconName::VolumeOff, theme::surface(), theme::text())
    };
    round_control("share-bar-sound", glyph, background, foreground)
        .size(px(BUTTON_SIZE))
        .tooltip(|window, cx| Tooltip::new("Include computer sound").build(window, cx))
        .on_click(move |_, _, cx| app.update(cx, |state, cx| state.set_call_share_sound(!sound, cx)))
}

fn pin_button(windows: &Entity<CallWindows>, pinned: bool) -> Stateful<Div> {
    let windows = windows.clone();
    let (glyph, background, foreground) = if pinned {
        (IconName::PinOff, theme::accent(), theme::on_accent())
    } else {
        (IconName::Pin, theme::surface(), theme::text())
    };
    round_control("share-bar-pin", glyph, background, foreground)
        .size(px(BUTTON_SIZE))
        .tooltip(move |window, cx| Tooltip::new(if pinned { "Unpin the bar" } else { "Keep the bar visible" }).build(window, cx))
        .on_click(move |_, _, cx| windows.update(cx, |windows, cx| windows.toggle_pin(cx)))
}

impl Render for ShareBarView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let windows = self.windows.read(cx);
        let wake = self.windows.clone();
        let wake_on_move = move |_: &MouseMoveEvent, _: &mut Window, cx: &mut App| wake.update(cx, |windows, cx| windows.wake_bar(cx));
        if windows.bar_collapsed() {
            return div()
                .id("share-bar-handle")
                .size_full()
                .on_mouse_move(wake_on_move)
                .child(div().size_full().rounded_b(px(HANDLE_RADIUS)).bg(theme::red()))
                .into_any_element();
        }
        let text = windows.share_text();
        let pinned = windows.pinned();
        let state = self.app.read(cx);
        let Some(call) = state.call.as_ref() else {
            return div().into_any_element();
        };
        h_flex()
            .id("share-bar")
            .size_full()
            .px(px(10.))
            .gap(px(6.))
            .items_center()
            .rounded_b(px(BAR_RADIUS))
            .bg(theme::surface_raised())
            .border_1()
            .border_color(theme::red())
            .text_color(theme::text())
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .on_mouse_move(wake_on_move)
            .child(div().size(px(8.)).flex_none().rounded_full().bg(theme::red()))
            .child(div().flex_1().min_w_0().truncate().child(text))
            .child(mute_button(&self.app, &call.model, "share-bar-mute", BUTTON_SIZE))
            .child(camera_button(&self.app, &call.model).size(px(BUTTON_SIZE)))
            .child(sound_button(&self.app, state.call_share_sound))
            .child(stop_button(&self.app))
            .child(pin_button(&self.windows, pinned))
            .into_any_element()
    }
}

pub struct ShareBorderView;

impl Render for ShareBorderView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(theme::red())
    }
}
