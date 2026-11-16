//! Calendar view settings on `AppState`: local preferences plus the mailbox work hours.

use app_base::ui_locale::tr;
use gpui_kit::Context;
use graph::MailboxWorkingHours;

use super::calendar_prefs::{CalendarPrefs, WorkHours, save_calendar_prefs};
use crate::app_state::AppState;

impl AppState {
    /// Change local calendar preferences, save them and redraw the visible range.
    pub fn update_calendar_prefs(
        &mut self,
        change: impl FnOnce(&mut CalendarPrefs),
        cx: &mut Context<Self>,
    ) {
        change(&mut self.calendar_prefs);
        save_calendar_prefs(&self.store, &self.calendar_prefs);
        // The mailbox language can move the first day of week.
        self.apply_regional_format();
        self.reanchor_calendar(cx);
        cx.notify();
    }

    /// Week start or work days may have moved: show the range that holds the same date.
    pub(crate) fn reanchor_calendar(&mut self, cx: &mut Context<Self>) {
        if let Some(anchor) = self.calendar_week.start {
            self.calendar_week.start = Some(self.calendar_span_start(anchor));
            self.request_calendar_scroll_to_now(cx);
            self.load_calendar_week(cx);
        }
    }

    /// Read the mailbox work hours once the session is there and cache them.
    pub(crate) fn load_work_hours(&mut self, cx: &mut Context<Self>) {
        if self.mode.demo {
            return;
        }
        let Some(engine) = self.engine.clone() else {
            return;
        };
        let receiver = crate::runtime::spawn(async move { engine.remote().working_hours().await });
        cx.spawn(async move |this, cx| {
            let Ok(result) = receiver.await else {
                return;
            };
            this.update(cx, |state, cx| match result {
                Ok(mailbox) => {
                    state.mailbox_time_zone = mailbox.time_zone.clone();
                    let hours = work_hours_from(&mailbox);
                    let locale = mailbox.locale.clone();
                    let prefs = &state.calendar_prefs;
                    if hours != prefs.work_hours || locale != prefs.mailbox_locale {
                        state.update_calendar_prefs(
                            |prefs| {
                                prefs.work_hours = hours;
                                prefs.mailbox_locale = locale;
                            },
                            cx,
                        );
                    }
                }
                Err(error) => crate::diagnostics::record(format!(
                    "calendar: mailbox work hours not read: {error}"
                )),
            })
            .ok();
        })
        .detach();
    }

    /// Save work days and hours to the mailbox; on failure the previous value returns.
    pub fn set_work_hours(&mut self, hours: WorkHours, cx: &mut Context<Self>) {
        let previous = self.calendar_prefs.work_hours.clone();
        if previous == hours {
            return;
        }
        let wanted = hours.clone();
        self.update_calendar_prefs(|prefs| prefs.work_hours = wanted, cx);
        if self.mode.demo {
            return;
        }
        let Some(engine) = self.engine.clone() else {
            self.revert_work_hours(previous, "no session".to_owned(), cx);
            return;
        };
        let mailbox = MailboxWorkingHours {
            days: hours.days,
            start: hours.start,
            end: hours.end,
            time_zone: self.mailbox_time_zone.clone(),
            locale: None,
        };
        let receiver =
            crate::runtime::spawn(async move { engine.remote().set_working_hours(&mailbox).await });
        cx.spawn(async move |this, cx| {
            let result = receiver
                .await
                .unwrap_or_else(|_| Err(graph::Error::Decode("mailbox task cancelled".into())));
            this.update(cx, |state, cx| {
                if let Err(error) = result {
                    state.revert_work_hours(previous, error.to_string(), cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn revert_work_hours(&mut self, previous: WorkHours, error: String, cx: &mut Context<Self>) {
        crate::diagnostics::record(format!("calendar: mailbox work hours not saved: {error}"));
        self.update_calendar_prefs(|prefs| prefs.work_hours = previous, cx);
        self.raise_notice(tr("settings.calendar.work_hours_not_saved"), None, cx);
    }
}

fn work_hours_from(mailbox: &MailboxWorkingHours) -> WorkHours {
    WorkHours {
        days: mailbox.days.clone(),
        start: mailbox.start,
        end: mailbox.end,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::Weekday;
    use gpui_kit::{AppContext as _, TestAppContext};
    use store::Store;

    use crate::app_state::{AppState, Mode};
    use crate::call::calendar_prefs::{DefaultView, WorkHours, load_calendar_prefs};
    use crate::call::week_grid::CalendarSpan;

    #[gpui_kit::test]
    fn the_week_start_follows_the_mailbox_language(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let store = Arc::new(Store::open_in_memory().unwrap());
        let app = cx.update(|cx| cx.new(|_| AppState::new(store, Mode::default())));
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state
                    .update_calendar_prefs(|prefs| prefs.mailbox_locale = Some("en-US".into()), cx);
                assert_eq!(state.calendar_layout().first_day, Weekday::Sun);
                state
                    .update_calendar_prefs(|prefs| prefs.mailbox_locale = Some("en-CH".into()), cx);
                assert_eq!(state.calendar_layout().first_day, Weekday::Mon);
                let prefs = crate::regional::RegionalPrefs {
                    first_day: Some(Weekday::Sat),
                    ..state.regional_prefs
                };
                state.set_regional_prefs(prefs, cx);
                assert_eq!(state.calendar_layout().first_day, Weekday::Sat);
            })
        });
    }

    #[gpui_kit::test]
    fn the_chosen_span_survives_a_restart(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let store = Arc::new(Store::open_in_memory().unwrap());
        let first = cx.update(|cx| cx.new(|_| AppState::new(store.clone(), Mode::default())));
        cx.update(|cx| {
            first.update(cx, |state, cx| {
                assert_eq!(state.calendar_week.span, CalendarSpan::WorkWeek);
                state.set_calendar_span(CalendarSpan::Month, cx);
                // A day opened from a month cell is temporary navigation.
                let today = chrono::Local::now().date_naive();
                state.show_calendar_day(today, cx);
            })
        });
        let second = cx.update(|cx| cx.new(|_| AppState::new(store.clone(), Mode::default())));
        cx.update(|cx| {
            second.update(cx, |state, cx| {
                assert_eq!(state.calendar_week.span, CalendarSpan::Month);
                state.update_calendar_prefs(|prefs| prefs.default_view = DefaultView::Week, cx);
                state.set_calendar_span(CalendarSpan::Day, cx);
            })
        });
        let third = cx.update(|cx| cx.new(|_| AppState::new(store, Mode::default())));
        cx.update(|cx| {
            third.update(cx, |state, _| {
                assert_eq!(state.calendar_week.span, CalendarSpan::Week)
            })
        });
    }

    #[gpui_kit::test]
    fn work_hours_fall_back_when_the_mailbox_cannot_be_written(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let store = Arc::new(Store::open_in_memory().unwrap());
        let app = cx.update(|cx| cx.new(|_| AppState::new(store.clone(), Mode::default())));
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                let mut hours = WorkHours::default();
                hours.days.retain(|day| *day != Weekday::Fri);
                state.set_work_hours(hours, cx);
                assert_eq!(state.calendar_prefs.work_hours, WorkHours::default());
                assert!(state.notice.is_some());
            })
        });
        assert_eq!(load_calendar_prefs(&store).work_hours, WorkHours::default());
    }
}
