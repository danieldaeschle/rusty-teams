use std::time::Instant;

use gpui_kit::assets::IconName;
use gpui_kit::component::h_flex;
use gpui_kit::*;

use super::call_mini_window::{
    ControlIds, WINDOW_RADIUS, chip, controls_row, self_view, speaker_layer, speaker_name_chip,
    speaker_speaking, tinted_chip,
};
use super::call_view::status_line;
use super::toolbar_placement::TimelineBounds;
use super::widgets::icon;
use crate::app_state::AppState;
use crate::theme;
use app_ui_kit::layout::ROW_PADDING;

const MINI_WIDTH: f32 = 256.;
const MINI_HEIGHT: f32 = 144.;
const MINI_RIGHT: f32 = 16.;
const MINI_BOTTOM: f32 = 40.;

pub struct CallMiniView {
    app: Entity<AppState>,
    timeline: TimelineBounds,
    _app_subscription: Subscription,
    _media_subscription: Option<Subscription>,
}

impl CallMiniView {
    pub fn new(app: Entity<AppState>, timeline: TimelineBounds, cx: &mut Context<Self>) -> Self {
        let app_subscription = cx.observe(&app, |_, _, cx| cx.notify());
        let media_subscription = app
            .read(cx)
            .call_media
            .clone()
            .map(|media| cx.observe(&media, |_, _, cx| cx.notify()));
        CallMiniView {
            app,
            timeline,
            _app_subscription: app_subscription,
            _media_subscription: media_subscription,
        }
    }
}

fn stop_sharing_pill(app: &Entity<AppState>) -> Stateful<Div> {
    let stop_app = app.clone();
    div()
        .id("call-mini-stop-sharing")
        .absolute()
        .top(px(8.))
        .right(px(8.))
        .h(px(24.))
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(4.))
        .rounded_full()
        .bg(theme::red())
        .text_color(theme::white())
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .hover(|button| button.opacity(0.85))
        .child(icon(IconName::ScreenShare, 14., theme::white()))
        .child("Stop")
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            stop_app.update(cx, |state, cx| state.stop_call_share(cx));
        })
}

impl Render for CallMiniView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.app.read(cx);
        let Some(call) = state.call.as_ref().filter(|call| !call.viewing) else {
            return div().into_any_element();
        };
        let model = &call.model;
        let now = Instant::now();
        let (status, _) = status_line(model, now);
        let viewport = window.viewport_size();
        let (right, bottom) = self.timeline.get().map_or(
            (px(MINI_RIGHT), px(MINI_BOTTOM)),
            |timeline| {
                (
                    viewport.width - timeline.right() + px(ROW_PADDING),
                    viewport.height - timeline.bottom() + px(MINI_RIGHT),
                )
            },
        );
        let sharing = model.local_share.is_some();
        let open_app = self.app.clone();
        let back_app = self.app.clone();
        let speaker_name = speaker_name_chip(state, model, now);
        let border = if sharing {
            theme::red()
        } else if speaker_speaking(model) {
            theme::green()
        } else {
            theme::border_strong()
        };
        let top_left = if sharing {
            tinted_chip("Sharing".to_owned(), theme::red())
        } else {
            chip(status)
        };
        div()
            .id("call-mini")
            .group("call-mini")
            .occlude()
            .absolute()
            .right(right)
            .bottom(bottom)
            .w(px(MINI_WIDTH))
            .h(px(MINI_HEIGHT))
            .overflow_hidden()
            .rounded(px(WINDOW_RADIUS))
            .bg(theme::surface())
            .border_2()
            .border_color(border)
            .text_color(theme::text())
            .shadow(theme::flyout_shadow())
            .cursor_pointer()
            .on_click(move |_, _, cx| {
                open_app.update(cx, |state, cx| state.show_call(true, cx));
            })
            .child(speaker_layer(state, model, now))
            .child(
                div()
                    .absolute()
                    .top(px(8.))
                    .left(px(8.))
                    .child(top_left),
            )
            .children(speaker_name.map(|name| {
                h_flex()
                    .absolute()
                    .bottom(px(6.))
                    .left(px(8.))
                    .child(name)
            }))
            .children(if sharing {
                Some(stop_sharing_pill(&self.app).into_any_element())
            } else {
                self_view(state, model, now)
            })
            .child(controls_row(
                &self.app,
                model,
                ControlIds {
                    mute: "call-mini-mute",
                    back: "call-mini-back",
                    leave: "call-mini-leave",
                },
                "call-mini",
                move |cx| back_app.update(cx, |state, cx| state.show_call(true, cx)),
            ))
            .into_any_element()
    }
}
