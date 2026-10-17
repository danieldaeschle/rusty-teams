use std::time::Instant;

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, tooltip::Tooltip, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::call_mini_window::{CONTROL_SIZE, WINDOW_RADIUS};
use super::call_view::{
    camera_button, leave_button, mute_button, plain_own_tile, plain_person_tile, round_control,
};
use crate::app_state::AppState;
use crate::call::share_overlay::{
    GALLERY_CONTROLS_HEIGHT, GALLERY_DIVIDER_HEIGHT, GALLERY_INSET, GALLERY_TILE_WIDTH,
    GALLERY_TITLE_HEIGHT, gallery_candidates,
};
use crate::call::{CallWindows, TileSize};
use crate::theme;

pub const SHOW_SPEAKER_ONLY: &str = "Show only active speaker";
pub const SHOW_MORE_PEOPLE: &str = "Show more people";
const MAXIMIZE_MEETING_WINDOW: &str = "Maximize meeting window";
const TITLE_BUTTON_SIZE: f32 = 24.;
const TITLE_PADDING: f32 = (GALLERY_TITLE_HEIGHT - TITLE_BUTTON_SIZE) / 2.;
const CONTROLS_GAP: f32 = 8.;

pub struct ShareGalleryView {
    windows: Entity<CallWindows>,
    app: Entity<AppState>,
    _windows_subscription: Subscription,
    _app_subscription: Subscription,
    _media_subscription: Option<Subscription>,
}

impl ShareGalleryView {
    pub fn new(windows: Entity<CallWindows>, cx: &mut Context<Self>) -> Self {
        let app = windows.read(cx).app().clone();
        let windows_subscription = cx.observe(&windows, |_, _, cx| cx.notify());
        let app_subscription = cx.observe(&app, |_, _, cx| cx.notify());
        let media_subscription = app
            .read(cx)
            .call_media
            .clone()
            .map(|media| cx.observe(&media, |_, _, cx| cx.notify()));
        ShareGalleryView {
            windows,
            app,
            _windows_subscription: windows_subscription,
            _app_subscription: app_subscription,
            _media_subscription: media_subscription,
        }
    }
}

pub(super) fn view_switch_button(
    windows: &Entity<CallWindows>,
    id: &'static str,
    glyph: IconName,
    tooltip: &'static str,
) -> Stateful<Div> {
    let windows = windows.clone();
    round_control(id, glyph, theme::surface_raised(), theme::text())
        .size(px(TITLE_BUTTON_SIZE))
        .tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx))
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            windows.update(cx, |windows, cx| windows.toggle_speaker_only(cx));
        })
}

fn maximize_button(windows: &Entity<CallWindows>) -> Stateful<Div> {
    let windows = windows.clone();
    round_control(
        "call-gallery-maximize",
        IconName::Maximize2,
        theme::surface_raised(),
        theme::text(),
    )
    .size(px(TITLE_BUTTON_SIZE))
    .tooltip(|window, cx| Tooltip::new(MAXIMIZE_MEETING_WINDOW).build(window, cx))
    .on_click(move |_, _, cx| {
        cx.stop_propagation();
        windows.update(cx, |windows, cx| windows.return_to_call(cx));
    })
}

fn title_bar(windows: &Entity<CallWindows>) -> Div {
    h_flex()
        .w_full()
        .h(px(GALLERY_TITLE_HEIGHT))
        .flex_none()
        .items_center()
        .gap(px(TITLE_PADDING))
        .pr(px(TITLE_PADDING))
        .child(
            div()
                .id("call-gallery-drag")
                .flex_1()
                .h_full()
                .window_control_area(WindowControlArea::Drag)
                .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move()),
        )
        .child(view_switch_button(
            windows,
            "call-gallery-speaker-only",
            IconName::User,
            SHOW_SPEAKER_ONLY,
        ))
        .child(maximize_button(windows))
}

fn divider(overflow: usize) -> Div {
    div()
        .relative()
        .w_full()
        .h(px(GALLERY_DIVIDER_HEIGHT))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(px(GALLERY_DIVIDER_HEIGHT / 2.))
                .h(px(1.))
                .bg(theme::border()),
        )
        .when(overflow > 0, |row| {
            row.child(
                div()
                    .px(px(8.))
                    .rounded_full()
                    .bg(theme::surface_raised())
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text_muted())
                    .child(format!("+{overflow}")),
            )
        })
}

impl Render for ShareGalleryView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let layout = self.windows.read(cx).gallery_layout();
        let state = self.app.read(cx);
        let Some(call) = state.call.as_ref() else {
            return div().into_any_element();
        };
        let model = &call.model;
        let now = Instant::now();
        let size = TileSize::sixteen_by_nine(GALLERY_TILE_WIDTH);
        let tiles: Vec<AnyElement> = gallery_candidates(model)
            .into_iter()
            .take(layout.tiles)
            .map(|tile| plain_person_tile(state, model, tile, size, now))
            .collect();
        v_flex()
            .id("call-gallery")
            .size_full()
            .overflow_hidden()
            .items_center()
            .gap(px(GALLERY_INSET))
            .rounded(px(WINDOW_RADIUS))
            .bg(theme::surface())
            .text_color(theme::text())
            .child(title_bar(&self.windows))
            .children(tiles)
            .when(layout.divider, |column| {
                column.child(divider(layout.overflow))
            })
            .when(layout.self_tile, |column| {
                column.child(plain_own_tile(state, model, size, now))
            })
            .child(
                h_flex()
                    .w_full()
                    .h(px(GALLERY_CONTROLS_HEIGHT))
                    .flex_none()
                    .mt_auto()
                    .items_center()
                    .justify_center()
                    .gap(px(CONTROLS_GAP))
                    .child(mute_button(
                        &self.app,
                        model,
                        "call-gallery-mute",
                        CONTROL_SIZE,
                    ))
                    .child(camera_button(&self.app, model).size(px(CONTROL_SIZE)))
                    .child(leave_button(&self.app, model, "call-gallery-leave", true)),
            )
            .into_any_element()
    }
}
