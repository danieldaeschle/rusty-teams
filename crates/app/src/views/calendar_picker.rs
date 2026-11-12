use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use store::CalendarAccounts;

use crate::app_state::AppState;
use crate::theme;
use crate::views::widgets::icon;

pub const PICKER_WIDTH: f32 = 220.;
const ROW_HEIGHT: f32 = 28.;

/// Teams-like calendar list: Outlook + each connected Google calendar with show toggles.
pub fn render(app: &Entity<AppState>, cx: &App) -> Div {
    let accounts = app.read(cx).calendar_accounts();
    let mut column = v_flex()
        .debug_selector(|| "shell/islands/calendar/picker".into())
        .w(px(PICKER_WIDTH))
        .flex_none()
        .h_full()
        .border_r_1()
        .border_color(theme::border())
        .bg(theme::surface())
        .px(px(12.))
        .py(px(12.))
        .gap(px(4.))
        .child(
            div()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text_muted())
                .mb(px(6.))
                .child("CALENDARS"),
        )
        .child(outlook_row(app, &accounts));
    for account in &accounts.google_accounts {
        column = column.child(
            div()
                .mt(px(8.))
                .mb(px(2.))
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text_muted())
                .truncate()
                .child(account.email.clone()),
        );
        for calendar in &account.calendars {
            let toggle_app = app.clone();
            let account_id = account.id.clone();
            let calendar_id = calendar.id.clone();
            let color = if calendar.primary {
                theme::accent()
            } else {
                theme::text_soft()
            };
            column = column.child(toggle_row(
                format!("calendar-picker-{}", calendar.id),
                calendar.summary.clone(),
                calendar.selected,
                color,
                move |cx| {
                    toggle_app.update(cx, |state, cx| {
                        state.toggle_google_calendar_selected(&account_id, &calendar_id, cx)
                    });
                },
            ));
        }
    }
    if !accounts.has_google() {
        column = column.child(
            div()
                .mt(px(8.))
                .text_size(px(12.))
                .text_color(theme::text_muted())
                .child("Connect Google in Settings › Calendar"),
        );
    }
    column
}

fn outlook_row(app: &Entity<AppState>, accounts: &CalendarAccounts) -> Stateful<Div> {
    let app = app.clone();
    toggle_row(
        "calendar-picker-outlook",
        "Outlook",
        accounts.outlook_selected,
        theme::accent(),
        move |cx| {
            app.update(cx, |state, cx| state.toggle_outlook_calendar_selected(cx));
        },
    )
}

fn toggle_row(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    on: bool,
    swatch: Hsla,
    on_click: impl Fn(&mut App) + 'static,
) -> Stateful<Div> {
    h_flex()
        .id(id)
        .h(px(ROW_HEIGHT))
        .flex_none()
        .w_full()
        .items_center()
        .gap(px(8.))
        .rounded(px(6.))
        .px(px(4.))
        .cursor_pointer()
        .hover(|row| row.bg(theme::popup_hover()))
        .on_click(move |_, _, cx| on_click(cx))
        .child(
            div()
                .size(px(14.))
                .flex_none()
                .rounded(px(3.))
                .bg(if on { swatch } else { theme::border() })
                .when(on, |box_| {
                    box_.flex().items_center().justify_center().child(icon(
                        IconName::Check,
                        10.,
                        theme::text(),
                    ))
                }),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(13.))
                .text_color(theme::text())
                .child(label.into()),
        )
}
