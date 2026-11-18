//! Calendar list pane left of the grid, laid out like Teams: heading, mini month,
//! Add calendar, and one collapsible group per account with show/hide boxes.

use std::rc::Rc;

use chrono::Local;
use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use store::{CalDavAccount, CalendarAccounts, GoogleAccount, IcsSubscription};

use super::calendar_mini_month::mini_month_grid;
use crate::app_state::AppState;
use crate::call::calendar_list::{GroupCheck, group_check, listed_calendars, offers_show_all};
use crate::call::calendar_prefs::OUTLOOK_GROUP;
use crate::call::week_grid::google_calendar_index;
use crate::i18n::tr;
use crate::settings_section::SettingsSection;
use crate::theme;
use crate::views::widgets::icon;

pub const PICKER_WIDTH: f32 = 236.;
pub const CALENDAR_PANE_TAG: &str = "shell/islands/calendar/picker";
pub const MINI_MONTH_TAG: &str = "shell/islands/calendar/picker/mini-month";
const ROW_HEIGHT: f32 = 28.;
const MINI_CELL: f32 = 30.;
const CHECK_SIZE: f32 = 14.;

pub fn render(app: &Entity<AppState>, cx: &App) -> Div {
    let state = app.read(cx);
    let accounts = state.calendar_accounts();
    let prefs = &state.calendar_prefs;
    let mut column = v_flex()
        .debug_selector(|| CALENDAR_PANE_TAG.into())
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
                .text_size(theme::text_heading())
                .font_weight(FontWeight::BOLD)
                .text_color(theme::text())
                .pb(px(8.))
                .child(tr("calendar.view.title")),
        )
        .child(mini_month(app, state))
        .child(add_calendar_row(app))
        .child(outlook_group(
            app,
            &accounts,
            prefs.group_collapsed(OUTLOOK_GROUP),
        ));
    for account in &accounts.google_accounts {
        column = column.child(google_group(
            app,
            &accounts,
            account,
            prefs.group_collapsed(&account.id),
            state.calendar_expanded_groups.contains(&account.id),
        ));
    }
    for account in &accounts.caldav_accounts {
        column = column.child(caldav_group(
            app,
            &accounts,
            account,
            prefs.group_collapsed(&account.id),
        ));
    }
    for subscription in &accounts.ics_subscriptions {
        column = column.child(ics_row(app, subscription));
    }
    column
}

/// Mini month with its own month arrows; a click shows the span around that day.
fn mini_month(app: &Entity<AppState>, state: &AppState) -> Div {
    let month = state.calendar_mini_month_shown();
    let (back, forward, pick) = (app.clone(), app.clone(), app.clone());
    v_flex()
        .debug_selector(|| MINI_MONTH_TAG.into())
        .pb(px(8.))
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .pb(px(4.))
                .child(
                    div()
                        .pl(px(4.))
                        .text_size(theme::text_label())
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::text())
                        .child(app_base::format::month_year_label(month)),
                )
                .child(
                    h_flex()
                        .child(
                            Button::new("calendar-mini-back")
                                .xsmall()
                                .ghost()
                                .icon(IconName::ChevronUp)
                                .tooltip(tr("calendar.view.previous_month_short"))
                                .on_click(move |_, _, cx| {
                                    back.update(cx, |state, cx| {
                                        state.move_calendar_mini_month(-1, cx)
                                    })
                                }),
                        )
                        .child(
                            Button::new("calendar-mini-forward")
                                .xsmall()
                                .ghost()
                                .icon(IconName::ChevronDown)
                                .tooltip(tr("calendar.view.next_month_short"))
                                .on_click(move |_, _, cx| {
                                    forward.update(cx, |state, cx| {
                                        state.move_calendar_mini_month(1, cx)
                                    })
                                }),
                        ),
                ),
        )
        .child(mini_month_grid(
            "calendar-mini",
            month,
            &state.calendar_dates(),
            state.calendar_layout().first_day,
            Local::now().date_naive(),
            MINI_CELL,
            Rc::new(move |date, _, cx| {
                pick.update(cx, |state, cx| state.show_calendar_date(date, cx))
            }),
        ))
}

fn add_calendar_row(app: &Entity<AppState>) -> Stateful<Div> {
    let app = app.clone();
    h_flex()
        .id("calendar-add-calendar")
        .h(px(ROW_HEIGHT))
        .flex_none()
        .w_full()
        .items_center()
        .gap(px(8.))
        .px(px(4.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|row| row.bg(theme::popup_hover()))
        .text_size(theme::text_label())
        .text_color(theme::accent_text())
        .child(icon(IconName::Plus, 14., theme::accent_text()))
        .child(tr("calendar.view.add_calendar"))
        .on_click(move |_, _, cx| {
            app.update(cx, |state, cx| {
                state.open_settings(SettingsSection::CalendarAccounts, cx)
            })
        })
}

fn outlook_group(app: &Entity<AppState>, accounts: &CalendarAccounts, collapsed: bool) -> Div {
    let on = accounts.outlook_selected;
    let (toggle_group, toggle_row) = (app.clone(), app.clone());
    let collapse = app.clone();
    group(
        format!("calendar-group-{OUTLOOK_GROUP}"),
        tr("calendar.view.my_calendars"),
        if on {
            GroupCheck::All
        } else {
            GroupCheck::None
        },
        collapsed,
        move |cx| toggle_group.update(cx, |state, cx| state.toggle_outlook_calendar_selected(cx)),
        move |cx| {
            collapse.update(cx, |state, cx| {
                state.update_calendar_prefs(|prefs| prefs.toggle_group(OUTLOOK_GROUP), cx)
            })
        },
    )
    .when(!collapsed, |group| {
        group.child(calendar_row(
            "calendar-picker-outlook",
            "Outlook".to_owned(),
            on,
            theme::calendar_color(0),
            move |cx| toggle_row.update(cx, |state, cx| state.toggle_outlook_calendar_selected(cx)),
        ))
    })
}

fn google_group(
    app: &Entity<AppState>,
    accounts: &CalendarAccounts,
    account: &GoogleAccount,
    collapsed: bool,
    expanded: bool,
) -> Div {
    let selected: Vec<bool> = account
        .calendars
        .iter()
        .map(|calendar| calendar.selected)
        .collect();
    let primary: Vec<bool> = account
        .calendars
        .iter()
        .map(|calendar| calendar.primary)
        .collect();
    let check = group_check(&selected);
    let (toggle_group, collapse, expand) = (app.clone(), app.clone(), app.clone());
    let (group_id, collapse_id, expand_id) =
        (account.id.clone(), account.id.clone(), account.id.clone());
    let mut body = group(
        format!("calendar-group-{}", account.id),
        account.email.clone(),
        check,
        collapsed,
        move |cx| {
            toggle_group.update(cx, |state, cx| {
                state.set_google_account_selected(&group_id, check != GroupCheck::All, cx)
            })
        },
        move |cx| {
            collapse.update(cx, |state, cx| {
                state.update_calendar_prefs(|prefs| prefs.toggle_group(&collapse_id), cx)
            })
        },
    );
    if collapsed {
        return body;
    }
    for index in listed_calendars(&selected, &primary, expanded) {
        let calendar = &account.calendars[index];
        let toggle = app.clone();
        let (account_id, calendar_id) = (account.id.clone(), calendar.id.clone());
        let color = google_calendar_index(accounts, &account.id, &calendar.id)
            .map_or_else(theme::accent, theme::calendar_color);
        body = body.child(calendar_row(
            format!("calendar-picker-{}-{}", account.id, calendar.id),
            calendar.summary.clone(),
            calendar.selected,
            color,
            move |cx| {
                toggle.update(cx, |state, cx| {
                    state.toggle_google_calendar_selected(&account_id, &calendar_id, cx)
                });
            },
        ));
    }
    if offers_show_all(account.calendars.len()) {
        body = body.child(
            div()
                .id(SharedString::from(format!(
                    "calendar-show-all-{}",
                    account.id
                )))
                .pl(px(28.))
                .h(px(ROW_HEIGHT))
                .flex_none()
                .flex()
                .items_center()
                .cursor_pointer()
                .text_size(theme::text_small())
                .text_color(theme::accent_text())
                .child(if expanded {
                    tr("calendar.view.show_fewer")
                } else {
                    tr("calendar.view.show_all")
                })
                .on_click(move |_, _, cx| {
                    expand.update(cx, |state, cx| {
                        state.toggle_calendar_group_expanded(&expand_id, cx)
                    })
                }),
        );
    }
    body
}

fn caldav_group(
    app: &Entity<AppState>,
    accounts: &CalendarAccounts,
    account: &CalDavAccount,
    collapsed: bool,
) -> Div {
    let selected: Vec<bool> = account
        .calendars
        .iter()
        .map(|calendar| calendar.selected)
        .collect();
    let check = group_check(&selected);
    let (collapse, group_id) = (app.clone(), account.id.clone());
    let mut body = group(
        format!("calendar-group-caldav-{}", account.id),
        format!("CalDAV · {}", account.display_name),
        check,
        collapsed,
        |_| {},
        move |cx| {
            collapse.update(cx, |state, cx| {
                state.update_calendar_prefs(|prefs| prefs.toggle_group(&group_id), cx)
            })
        },
    );
    if collapsed {
        return body;
    }
    for calendar in &account.calendars {
        let toggle = app.clone();
        let (account_id, calendar_id) = (account.id.clone(), calendar.id.clone());
        let color_index = accounts
            .google_accounts
            .iter()
            .map(|account| account.calendars.len())
            .sum::<usize>()
            + 1
            + accounts
                .caldav_accounts
                .iter()
                .flat_map(|account| account.calendars.iter().map(|c| c.id.as_str()))
                .position(|id| id == calendar.id)
                .unwrap_or(0);
        body = body.child(calendar_row(
            format!("calendar-picker-caldav-{}", calendar.id),
            calendar.summary.clone(),
            calendar.selected,
            theme::calendar_color(color_index),
            move |cx| {
                toggle.update(cx, |state, cx| {
                    state.toggle_caldav_calendar_selected(&account_id, &calendar_id, cx)
                });
            },
        ));
    }
    body
}

fn ics_row(app: &Entity<AppState>, subscription: &IcsSubscription) -> Stateful<Div> {
    let toggle = app.clone();
    let id = subscription.id.clone();
    calendar_row(
        format!("calendar-picker-ics-{}", subscription.id),
        subscription.summary.clone(),
        subscription.selected,
        theme::calendar_color(8),
        move |cx| {
            toggle.update(cx, |state, cx| {
                state.toggle_ics_subscription_selected(&id, cx)
            })
        },
    )
}

/// Group header: chevron to collapse, a check box for the whole group, the title.
fn group(
    id: String,
    title: String,
    check: GroupCheck,
    collapsed: bool,
    toggle: impl Fn(&mut App) + 'static,
    collapse: impl Fn(&mut App) + 'static,
) -> Div {
    v_flex().w_full().pt(px(6.)).child(
        h_flex()
            .h(px(ROW_HEIGHT))
            .flex_none()
            .w_full()
            .items_center()
            .gap(px(6.))
            .child(
                div()
                    .id(SharedString::from(format!("{id}-chevron")))
                    .flex_none()
                    .cursor_pointer()
                    .child(icon(
                        if collapsed {
                            IconName::ChevronRight
                        } else {
                            IconName::ChevronDown
                        },
                        14.,
                        theme::text_muted(),
                    ))
                    .on_click(move |_, _, cx| collapse(cx)),
            )
            .child(
                div()
                    .id(SharedString::from(format!("{id}-check")))
                    .flex_none()
                    .cursor_pointer()
                    .child(check_box(check, theme::accent()))
                    .on_click(move |_, _, cx| toggle(cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(theme::text_label())
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text())
                    .child(title),
            ),
    )
}

fn calendar_row(
    id: impl Into<ElementId>,
    label: String,
    on: bool,
    color: Hsla,
    on_click: impl Fn(&mut App) + 'static,
) -> Stateful<Div> {
    h_flex()
        .id(id)
        .h(px(ROW_HEIGHT))
        .flex_none()
        .w_full()
        .items_center()
        .gap(px(8.))
        .pl(px(24.))
        .pr(px(4.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|row| row.bg(theme::popup_hover()))
        .on_click(move |_, _, cx| on_click(cx))
        .child(check_box(
            if on {
                GroupCheck::All
            } else {
                GroupCheck::None
            },
            color,
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(theme::text_label())
                .text_color(theme::text())
                .child(label),
        )
}

/// Check box in the calendar's colour: filled with a tick when on, a dash when partly on.
fn check_box(check: GroupCheck, color: Hsla) -> Div {
    div()
        .size(px(CHECK_SIZE))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(3.))
        .map(|check_box| match check {
            GroupCheck::All => {
                check_box
                    .bg(color)
                    .child(icon(IconName::Check, 10., theme::on_accent()))
            }
            GroupCheck::Some => check_box.bg(color.opacity(0.5)).child(
                div()
                    .w(px(8.))
                    .h(px(2.))
                    .rounded_full()
                    .bg(theme::on_accent()),
            ),
            // Lines stay soft: an empty box is a light fill of the calendar colour.
            GroupCheck::None => check_box.bg(color.opacity(0.22)),
        })
}
