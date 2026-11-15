use gpui_kit::ClipboardItem;
use gpui_kit::component::v_flex;
use gpui_kit::*;
use store::{CalendarProvider, CalendarRef};

use std::collections::HashMap;

use app_base::format::{WeekdayWidth, clock_format, regional_first_day, weekday_name};
use chrono::{NaiveTime, Weekday};
use gpui_kit::component::h_flex;
use gpui_kit::prelude::FluentBuilder as _;

use super::rows::{
    Choice, caption, control_row, select, select_with_width, text_button, toggle_row,
};
use crate::app_state::AppState;
use crate::call::calendar_prefs::{CalendarPrefs, DefaultView, TimeScale, WeekendWorkWeek};
use crate::i18n::tr;
use crate::theme;

pub fn render(app: &Entity<AppState>, cx: &App) -> Div {
    let accounts = app.read(cx).calendar_accounts();
    let pending = app.read(cx).google_connect_pending().is_some();
    let prefs = app.read(cx).calendar_prefs.clone();
    let mut body = v_flex()
        .w_full()
        .child(caption(tr("settings.calendar.section_view")))
        .children(view_rows(app, &prefs))
        .child(caption(tr("settings.calendar.section_google")))
        .child(connect_row(app, accounts.has_google(), pending));
    for account in &accounts.google_accounts {
        body = body.child(account_block(app, account, &accounts));
    }
    body = body
        .child(caption(tr("settings.calendar.section_home")))
        .child(home_choices(app, &accounts))
        .child(caption(tr("settings.calendar.section_write")));
    if accounts.home.provider != CalendarProvider::Outlook {
        let write_app = app.clone();
        let target = CalendarRef::outlook();
        let selected = accounts
            .write_targets
            .iter()
            .any(|existing| existing.key() == target.key());
        body = body.child(
            toggle_row(
                "settings-write-outlook",
                tr("settings.calendar.outlook_mailbox"),
                tr("settings.calendar.dual_write_when_google_home"),
                theme::text_muted(),
                selected,
            )
            .on_click(move |_, _, cx| {
                write_app.update(cx, |state, cx| {
                    state.toggle_write_target(target.clone(), cx)
                })
            }),
        );
    } else if accounts.google_accounts.is_empty() {
        body = body.child(control_row(
            tr("settings.calendar.no_extra_targets"),
            tr("settings.calendar.connect_then_dual_write"),
            div(),
        ));
    }
    body
}

fn connect_row(app: &Entity<AppState>, connected: bool, pending: bool) -> Div {
    let app = app.clone();
    if connected {
        control_row(
            tr("settings.calendar.google_title"),
            tr("settings.calendar.connected_detail"),
            div(),
        )
    } else if pending {
        let reopen = app.clone();
        let copy = app.clone();
        let cancel = app.clone();
        control_row(
            tr("settings.calendar.google_title"),
            tr("settings.calendar.finish_in_browser"),
            div()
                .flex()
                .gap_2()
                .child(
                    text_button(
                        "settings-google-reopen",
                        tr("settings.calendar.open_browser_again"),
                    )
                    .on_click(move |_, _, cx| {
                        reopen.update(cx, |state, cx| state.reopen_google_connect(cx))
                    }),
                )
                .child(
                    text_button("settings-google-copy", tr("settings.calendar.copy_link"))
                        .on_click(move |_, _, cx| {
                            if let Some(url) =
                                copy.read(cx).google_connect_pending().map(str::to_owned)
                            {
                                cx.write_to_clipboard(ClipboardItem::new_string(url));
                            }
                        }),
                )
                .child(
                    text_button("settings-google-cancel", tr("common.cancel")).on_click(
                        move |_, _, cx| {
                            cancel.update(cx, |state, cx| state.cancel_google_connect(cx))
                        },
                    ),
                ),
        )
    } else {
        let connect = app.clone();
        control_row(
            tr("settings.calendar.google_title"),
            tr("settings.calendar.connect_hint"),
            text_button(
                "settings-google-connect",
                tr("settings.calendar.connect_button"),
            )
            .on_click(move |_, _, cx| {
                connect.update(cx, |state, cx| state.connect_google_calendar(cx))
            }),
        )
    }
}

fn account_block(
    app: &Entity<AppState>,
    account: &store::GoogleAccount,
    accounts: &store::CalendarAccounts,
) -> Div {
    let disconnect_app = app.clone();
    let account_id = account.id.clone();
    let mut block = v_flex().w_full().child(control_row(
        account.email.clone(),
        tr("settings.calendar.google_account"),
        text_button(
            format!("settings-google-disconnect-{}", account.id),
            tr("settings.calendar.disconnect"),
        )
        .on_click(move |_, _, cx| {
            disconnect_app.update(cx, |state, cx| {
                state.disconnect_google_account(&account_id, cx)
            })
        }),
    ));
    for calendar in &account.calendars {
        let toggle_app = app.clone();
        let account_id = account.id.clone();
        let calendar_id = calendar.id.clone();
        let write_ref = CalendarRef::google(&account.id, &calendar.id);
        let is_home = accounts.home.key() == write_ref.key();
        let is_write = accounts
            .write_targets
            .iter()
            .any(|target| target.key() == write_ref.key());
        block = block.child(
            toggle_row(
                format!("settings-google-cal-{}", calendar.id),
                calendar.summary.clone(),
                if calendar.primary {
                    tr("settings.calendar.primary")
                } else {
                    tr("settings.calendar.show_in_grid")
                },
                theme::text_muted(),
                calendar.selected,
            )
            .on_click(move |_, _, cx| {
                toggle_app.update(cx, |state, cx| {
                    state.toggle_google_calendar_selected(&account_id, &calendar_id, cx)
                })
            }),
        );
        if !is_home {
            let write_app = app.clone();
            let target = write_ref;
            block = block.child(
                toggle_row(
                    format!("settings-google-write-{}", calendar.id),
                    tr_args_write(&calendar.summary),
                    "",
                    theme::text_muted(),
                    is_write,
                )
                .on_click(move |_, _, cx| {
                    write_app.update(cx, |state, cx| {
                        state.toggle_write_target(target.clone(), cx)
                    })
                }),
            );
        }
    }
    block
}

fn tr_args_write(summary: &str) -> String {
    let mut args = std::collections::HashMap::new();
    args.insert("name".to_owned(), summary.to_owned());
    crate::i18n::tr_args("settings.calendar.also_write_to", &args)
}

fn home_choices(app: &Entity<AppState>, accounts: &store::CalendarAccounts) -> Div {
    let mut choices = v_flex().w_full();
    let outlook_app = app.clone();
    let outlook_home = accounts.home.provider == CalendarProvider::Outlook;
    choices = choices.child(
        toggle_row(
            "settings-home-outlook",
            tr("settings.calendar.outlook_mailbox"),
            tr("settings.calendar.outlook_home_detail"),
            theme::text_muted(),
            outlook_home,
        )
        .on_click(move |_, _, cx| {
            outlook_app.update(cx, |state, cx| {
                state.set_home_calendar(CalendarRef::outlook(), cx)
            })
        }),
    );
    for account in &accounts.google_accounts {
        for calendar in &account.calendars {
            let home_app = app.clone();
            let target = CalendarRef::google(&account.id, &calendar.id);
            let selected = accounts.home.key() == target.key();
            choices = choices.child(
                toggle_row(
                    format!("settings-home-google-{}", calendar.id),
                    format!(
                        "{} · {}",
                        tr("settings.calendar.google_label"),
                        calendar.summary
                    ),
                    account.email.clone(),
                    theme::text_muted(),
                    selected,
                )
                .on_click(move |_, _, cx| {
                    home_app.update(cx, |state, cx| state.set_home_calendar(target.clone(), cx))
                }),
            );
        }
    }
    choices
}

const FIRST_DAY_CHOICES: [Weekday; 3] = [Weekday::Mon, Weekday::Sun, Weekday::Sat];
const CHIP_SIZE: f32 = 32.;
const TIME_SELECT_WIDTH: f32 = 104.;

/// Default view, first day, work days and hours, time scale, weekend Work week.
fn view_rows(app: &Entity<AppState>, prefs: &CalendarPrefs) -> Vec<Div> {
    vec![
        control_row(
            tr("settings.calendar.default_view"),
            tr("settings.calendar.default_view_detail"),
            select(
                "settings-calendar-default-view",
                default_view_label(prefs.default_view),
                false,
                DefaultView::ALL
                    .into_iter()
                    .map(|view| {
                        prefs_choice(
                            app,
                            default_view_label(view),
                            prefs.default_view == view,
                            move |prefs| prefs.default_view = view,
                        )
                    })
                    .collect(),
            ),
        ),
        control_row(
            tr("settings.calendar.first_day"),
            tr("settings.calendar.first_day_detail"),
            select(
                "settings-calendar-first-day",
                first_day_label(prefs.first_day),
                false,
                std::iter::once(None)
                    .chain(FIRST_DAY_CHOICES.into_iter().map(Some))
                    .map(|day| {
                        prefs_choice(
                            app,
                            first_day_label(day),
                            prefs.first_day == day,
                            move |prefs| prefs.first_day = day,
                        )
                    })
                    .collect(),
            ),
        ),
        control_row(
            tr("settings.calendar.work_days"),
            tr("settings.calendar.mailbox_detail"),
            work_day_chips(app, prefs),
        ),
        control_row(
            tr("settings.calendar.work_hours"),
            tr("settings.calendar.mailbox_detail"),
            h_flex()
                .gap(px(8.))
                .child(work_time_select(app, prefs, true))
                .child(work_time_select(app, prefs, false)),
        ),
        control_row(
            tr("settings.calendar.time_scale"),
            tr("settings.calendar.time_scale_detail"),
            select(
                "settings-calendar-time-scale",
                time_scale_label(prefs.time_scale),
                false,
                TimeScale::ALL
                    .into_iter()
                    .map(|scale| {
                        prefs_choice(
                            app,
                            time_scale_label(scale),
                            prefs.time_scale == scale,
                            move |prefs| prefs.time_scale = scale,
                        )
                    })
                    .collect(),
            ),
        ),
        control_row(
            tr("settings.calendar.weekend"),
            tr("settings.calendar.weekend_detail"),
            select(
                "settings-calendar-weekend",
                weekend_label(prefs.weekend_work_week),
                false,
                [WeekendWorkWeek::NextWeek, WeekendWorkWeek::ThisWeek]
                    .into_iter()
                    .map(|rule| {
                        prefs_choice(
                            app,
                            weekend_label(rule),
                            prefs.weekend_work_week == rule,
                            move |prefs| prefs.weekend_work_week = rule,
                        )
                    })
                    .collect(),
            ),
        ),
    ]
}

fn prefs_choice(
    app: &Entity<AppState>,
    label: String,
    checked: bool,
    change: impl Fn(&mut CalendarPrefs) + 'static,
) -> Choice {
    let app = app.clone();
    let change = std::rc::Rc::new(change);
    Choice::new(label, checked, move |cx| {
        let change = change.clone();
        app.update(cx, |state, cx| {
            state.update_calendar_prefs(|prefs| change(prefs), cx)
        });
    })
}

fn default_view_label(view: DefaultView) -> String {
    view.span()
        .map_or_else(|| tr("settings.calendar.last_used"), |span| span.label())
}

fn first_day_label(day: Option<Weekday>) -> String {
    match day {
        Some(day) => weekday_name(day, WeekdayWidth::Full),
        None => {
            let mut args = HashMap::new();
            args.insert(
                "day".to_owned(),
                weekday_name(regional_first_day(), WeekdayWidth::Full),
            );
            crate::i18n::tr_args("settings.calendar.first_day_regional", &args)
        }
    }
}

fn time_scale_label(scale: TimeScale) -> String {
    let mut args = HashMap::new();
    args.insert("count".to_owned(), scale.minutes().to_string());
    crate::i18n::tr_args("settings.calendar.minutes", &args)
}

fn weekend_label(rule: WeekendWorkWeek) -> String {
    match rule {
        WeekendWorkWeek::NextWeek => tr("settings.calendar.weekend_next"),
        WeekendWorkWeek::ThisWeek => tr("settings.calendar.weekend_this"),
    }
}

/// Seven day chips from the first day of week; the last work day cannot be turned off.
fn work_day_chips(app: &Entity<AppState>, prefs: &CalendarPrefs) -> Div {
    let layout = prefs.layout(regional_first_day());
    h_flex()
        .gap(px(4.))
        .children(layout.weekdays().into_iter().map(|day| {
            let on = prefs.work_hours.is_work_day(day);
            let last = on && prefs.work_hours.days.len() == 1;
            let app = app.clone();
            let hours = prefs.work_hours.clone();
            div()
                .id(SharedString::from(format!(
                    "settings-calendar-work-day-{day}"
                )))
                .size(px(CHIP_SIZE))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .border_1()
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .map(|chip| {
                    if on {
                        chip.bg(theme::accent())
                            .border_color(theme::accent())
                            .text_color(theme::on_accent())
                    } else {
                        chip.border_color(theme::border_strong())
                            .text_color(theme::text_soft())
                    }
                })
                .when(!last, |chip| {
                    chip.cursor_pointer().on_click(move |_, _, cx| {
                        let mut hours = hours.clone();
                        if on {
                            hours.days.retain(|existing| *existing != day);
                        } else {
                            hours.days.push(day);
                            hours.days.sort_by_key(Weekday::num_days_from_monday);
                        }
                        app.update(cx, |state, cx| state.set_work_hours(hours, cx));
                    })
                })
                .child(weekday_name(day, WeekdayWidth::Short))
        }))
}

/// Start or end of work in 30-minute steps; end choices stay after the start.
fn work_time_select(
    app: &Entity<AppState>,
    prefs: &CalendarPrefs,
    start: bool,
) -> impl IntoElement {
    let hours = prefs.work_hours.clone();
    let current = if start { hours.start } else { hours.end };
    let choices = (0..48)
        .filter_map(|step| NaiveTime::from_hms_opt(step / 2, (step % 2) * 30, 0))
        .filter(|time| {
            if start {
                *time < hours.end
            } else {
                *time > hours.start
            }
        })
        .map(|time| {
            let app = app.clone();
            let mut wanted = hours.clone();
            if start {
                wanted.start = time;
            } else {
                wanted.end = time;
            }
            Choice::new(
                time.format(clock_format()).to_string(),
                time == current,
                move |cx| {
                    let wanted = wanted.clone();
                    app.update(cx, |state, cx| state.set_work_hours(wanted, cx));
                },
            )
        })
        .collect();
    select_with_width(
        if start {
            "settings-calendar-work-start"
        } else {
            "settings-calendar-work-end"
        },
        current.format(clock_format()).to_string(),
        TIME_SELECT_WIDTH,
        false,
        choices,
    )
}
