use chrono::{Local, Offset, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::center::NotificationCenter;
use super::layout::{AVATAR_GAP, TOAST_PADDING};
use super::ring_view::{RING_AVATAR, answer_button, toast_radius};
use crate::app_state::AppState;
use crate::call::ReminderPhase;
use crate::call::meeting_subtitle;
use crate::theme;
use crate::views::widgets::icon;

const MEETING_ICON: f32 = 22.;
const LIVE_DOT: f32 = 7.;
const CLOSE_BUTTON: f32 = 24.;
const CLOSE_INSET: f32 = 8.;
const CLOSE_CLEARANCE: f32 = 20.;

pub struct MeetingToastView {
    center: Entity<NotificationCenter>,
    app: Entity<AppState>,
    toast_id: u64,
    _subscriptions: [Subscription; 2],
}

impl MeetingToastView {
    pub fn new(center: Entity<NotificationCenter>, toast_id: u64, cx: &mut Context<Self>) -> Self {
        let app = center.read(cx).app().clone();
        let subscriptions = [
            cx.observe(&app, |_, _, cx| cx.notify()),
            cx.observe(&center, |_, _, cx| cx.notify()),
        ];
        MeetingToastView {
            center,
            app,
            toast_id,
            _subscriptions: subscriptions,
        }
    }
}

impl Render for MeetingToastView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(toast) = self.app.read(cx).meeting_toasts.get(self.toast_id).cloned() else {
            return div().size_full().bg(theme::popup()).into_any_element();
        };
        window.set_window_title(&toast.subject);
        let subtitle = meeting_subtitle(&toast, Utc::now(), Local::now().offset().fix());
        let toast_id = self.toast_id;
        let join_center = self.center.clone();
        let snooze_app = self.app.clone();
        let dismiss_app = self.app.clone();
        let snooze = answer_button(
            "meeting-snooze",
            "Snooze",
            IconName::AlarmClock,
            theme::popup(),
        )
        .border_1()
        .border_color(theme::border_strong())
        .text_color(theme::text())
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            snooze_app.update(cx, |state, cx| state.snooze_meeting_toast(toast_id, cx));
        });
        let dismiss = div()
            .id("meeting-dismiss")
            .absolute()
            .top(px(CLOSE_INSET))
            .right(px(CLOSE_INSET))
            .size(px(CLOSE_BUTTON))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(|button| button.bg(theme::row_hover()))
            .child(icon(IconName::Close, 14., theme::text_muted()))
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                dismiss_app.update(cx, |state, cx| state.dismiss_meeting_toast(toast_id, cx));
            });
        let join = answer_button("meeting-join", "Join", IconName::Video, theme::accent())
            .text_color(theme::on_accent())
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                join_center.update(cx, |center, cx| center.join_meeting_toast(toast_id, cx));
            });
        let live = toast.phase == ReminderPhase::Live;
        let buttons = if toast.phase == ReminderPhase::Soon {
            h_flex().mt(px(8.)).gap(px(6.)).child(snooze).child(join)
        } else {
            h_flex().mt(px(8.)).gap(px(6.)).child(join)
        };
        div()
            .id(("meeting-toast", toast_id))
            .size_full()
            .rounded(toast_radius(window))
            .bg(theme::popup())
            .overflow_hidden()
            .child(
                div().size_full().p(px(TOAST_PADDING)).child(
                    h_flex()
                        .size_full()
                        .gap(px(AVATAR_GAP))
                        .items_start()
                        .child(
                            div()
                                .size(px(RING_AVATAR))
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .bg(theme::border_strong())
                                .child(icon(IconName::Video, MEETING_ICON, theme::text())),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .pr(px(CLOSE_CLEARANCE))
                                .gap(px(2.))
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(15.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme::text())
                                        .child(toast.subject.clone()),
                                )
                                .child(
                                    h_flex()
                                        .gap(px(6.))
                                        .items_center()
                                        .text_size(px(12.5))
                                        .text_color(theme::text_muted())
                                        .when(live, |row| {
                                            row.child(
                                                div()
                                                    .size(px(LIVE_DOT))
                                                    .flex_none()
                                                    .rounded_full()
                                                    .bg(theme::green()),
                                            )
                                        })
                                        .child(subtitle),
                                )
                                .child(buttons),
                        ),
                ),
            )
            .child(dismiss)
            .into_any_element()
    }
}
