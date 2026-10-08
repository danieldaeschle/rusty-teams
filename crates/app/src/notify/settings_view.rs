use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::center::NotificationCenter;
use super::rules::Settings;
use crate::theme;

type Accessor = fn(&mut Settings) -> &mut bool;

struct Row {
    title: &'static str,
    detail: &'static str,
    field: Accessor,
}

const ROWS: [Row; 5] = [
    Row {
        title: "Sound",
        detail: "Messages and mentions",
        field: |settings| &mut settings.sound,
    },
    Row {
        title: "Mentions only",
        detail: "Direct messages and @mentions",
        field: |settings| &mut settings.mentions_only,
    },
    Row {
        title: "Show preview",
        detail: "Off: only \"New message\"",
        field: |settings| &mut settings.preview,
    },
    Row {
        title: "Flash taskbar",
        detail: "On new messages, until you open the window",
        field: |settings| &mut settings.flash,
    },
    Row {
        title: "Minimize to tray on close",
        detail: "The title bar X hides the window",
        field: |settings| &mut settings.close_to_tray,
    },
];

pub struct SettingsView {
    center: Entity<NotificationCenter>,
    _subscription: Subscription,
}

impl SettingsView {
    pub const WIDTH: f32 = 420.;
    pub const HEIGHT: f32 = 460.;

    pub fn new(center: Entity<NotificationCenter>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&center, |_, _, cx| cx.notify());
        SettingsView {
            center,
            _subscription: subscription,
        }
    }
}

fn switch(on: bool) -> Div {
    div()
        .w(px(36.))
        .h(px(20.))
        .flex_none()
        .rounded_full()
        .relative()
        .bg(if on {
            theme::accent()
        } else {
            theme::border_strong()
        })
        .child(
            div()
                .absolute()
                .top(px(2.))
                .left(px(if on { 18. } else { 2. }))
                .size(px(16.))
                .rounded_full()
                .bg(theme::text()),
        )
}

fn button(label: &'static str) -> Stateful<Div> {
    div()
        .id(label)
        .h(px(32.))
        .px(px(14.))
        .flex()
        .items_center()
        .rounded(px(8.))
        .border_1()
        .border_color(theme::border_strong())
        .cursor_pointer()
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text())
        .hover(|button| button.bg(theme::row_hover()))
        .child(label)
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut current = self.center.read(cx).settings().clone();
        let corner = current.corner;
        let rows = ROWS.iter().enumerate().map(|(index, row)| {
            let on = *(row.field)(&mut current);
            let center = self.center.clone();
            let field = row.field;
            h_flex()
                .id(("setting", index))
                .w_full()
                .px(px(20.))
                .py(px(10.))
                .gap(px(12.))
                .items_center()
                .cursor_pointer()
                .hover(|row| row.bg(theme::row_hover()))
                .on_click(move |_, _, cx| {
                    center.update(cx, |center, cx| {
                        center.update_settings(cx, |settings| {
                            let value = field(settings);
                            *value = !*value;
                        })
                    });
                })
                .child(
                    v_flex()
                        .flex_1()
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::text())
                                .child(row.title),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(theme::text_muted())
                                .child(row.detail),
                        ),
                )
                .child(switch(on))
        });
        let position_center = self.center.clone();
        let test_center = self.center.clone();
        v_flex()
            .size_full()
            .bg(theme::background())
            .pt(px(12.))
            .children(rows)
            .child(
                h_flex()
                    .w_full()
                    .px(px(20.))
                    .py(px(10.))
                    .gap(px(12.))
                    .items_center()
                    .child(
                        v_flex()
                            .flex_1()
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::text())
                                    .child("Position"),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(theme::text_muted())
                                    .child("Corner of the main screen"),
                            ),
                    )
                    .child(button(corner.label()).on_click(move |_, _, cx| {
                        position_center.update(cx, |center, cx| {
                            center.update_settings(cx, |settings| {
                                settings.corner = settings.corner.next()
                            })
                        });
                    })),
            )
            .child(
                h_flex()
                    .w_full()
                    .px(px(20.))
                    .pt(px(12.))
                    .child(button("Send test notification").on_click(move |_, _, cx| {
                        test_center.update(cx, |center, cx| center.send_test_notification(cx));
                    })),
            )
    }
}
