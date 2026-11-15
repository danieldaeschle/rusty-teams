use std::time::Instant;

use chatsvc::{Calendar, CalendarEvent, CalendarSource, ResponseType, ShowAs};
use chrono::{Duration, FixedOffset, Local, NaiveDate, Offset, Weekday};
use gpui_kit::Context;

use super::agenda::demo_events;
use super::calendar::demo_anchor;
use super::calendar_prefs::save_calendar_prefs;
use super::week_grid::{
    CalendarSpan, WeekLayout, dates_range, local_midnight, visible_dates, week_start,
};
use crate::app_state::AppState;

pub const CALENDAR_LOAD_FAILURE: &str = "We couldn't load your calendar.";

/// One calendar source that did not load for the visible range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarLoadFailure {
    pub source: CalendarSource,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct CalendarWeek {
    pub start: Option<NaiveDate>,
    pub span: CalendarSpan,
    pub events: Vec<CalendarEvent>,
    pub loading: bool,
    /// Sources that failed on the last load; cleared by the next load.
    pub failures: Vec<CalendarLoadFailure>,
    /// The calendar section is on screen (drives refresh on the poll interval).
    pub visible: bool,
    /// A load was asked for before the session existed; run it once it does.
    pending: bool,
    request: u64,
}

impl CalendarWeek {
    pub fn with_span(span: CalendarSpan) -> Self {
        CalendarWeek {
            span,
            ..CalendarWeek::default()
        }
    }

    /// True once when a skipped load should run now.
    pub(crate) fn take_pending(&mut self) -> bool {
        std::mem::take(&mut self.pending)
    }
}

impl AppState {
    /// Week layout from the calendar settings and the regional first day.
    pub fn calendar_layout(&self) -> WeekLayout {
        self.calendar_prefs
            .layout(app_base::format::regional_first_day())
    }

    pub fn calendar_start(&self) -> NaiveDate {
        self.calendar_week
            .start
            .unwrap_or_else(|| self.calendar_span_start(Local::now().date_naive()))
    }

    /// Start of the visible span's range that contains `date`.
    pub fn calendar_span_start(&self, date: NaiveDate) -> NaiveDate {
        self.calendar_week
            .span
            .start_for(date, &self.calendar_layout())
    }

    /// The dates the grid draws, in column order.
    pub fn calendar_dates(&self) -> Vec<NaiveDate> {
        visible_dates(
            self.calendar_week.span,
            self.calendar_start(),
            &self.calendar_layout(),
        )
    }

    pub fn calendar_shows_today(&self) -> bool {
        self.calendar_dates().contains(&Local::now().date_naive())
    }

    pub fn open_calendar(&mut self, cx: &mut Context<Self>) {
        self.calendar_week.visible = true;
        self.calendar_week.start = Some(self.calendar_start());
        self.request_calendar_scroll_to_now(cx);
        self.load_calendar_week(cx);
    }

    /// The user left the calendar section; stop refreshing its range.
    pub fn close_calendar(&mut self) {
        self.calendar_week.visible = false;
    }

    /// Reload the visible range after an agenda poll while the calendar is on screen.
    pub(crate) fn refresh_visible_calendar(&mut self, cx: &mut Context<Self>) {
        if self.calendar_week.visible && self.calendar_week.start.is_some() {
            self.load_calendar_week(cx);
        }
    }

    /// Retry from the inline notice after a source failed.
    pub fn retry_calendar_week(&mut self, cx: &mut Context<Self>) {
        self.load_calendar_week(cx);
    }

    /// Run a grid load that was skipped because no session existed yet.
    pub(crate) fn run_pending_calendar_load(&mut self, cx: &mut Context<Self>) {
        if self.calendar_week.take_pending() {
            self.load_calendar_week(cx);
        }
    }

    pub fn shift_calendar_week(&mut self, steps: i32, cx: &mut Context<Self>) {
        let span = self.calendar_week.span;
        let layout = self.calendar_layout();
        self.calendar_week.start = Some(span.shifted(self.calendar_start(), steps, &layout));
        self.request_calendar_scroll_to_now(cx);
        self.load_calendar_week(cx);
    }

    pub fn calendar_to_today(&mut self, cx: &mut Context<Self>) {
        self.calendar_week.start = None;
        self.open_calendar(cx);
    }

    pub fn request_calendar_scroll_to_now(&mut self, cx: &mut Context<Self>) {
        if !crate::call::calendar_now::shows_hour_grid(self.calendar_week.span) {
            return;
        }
        self.calendar_scroll_to_now = self.calendar_scroll_to_now.wrapping_add(1);
        cx.notify();
    }

    /// Span chosen from the view menu; remembered as the last used span.
    pub fn set_calendar_span(&mut self, span: CalendarSpan, cx: &mut Context<Self>) {
        let today = Local::now().date_naive();
        let start = self.calendar_start();
        let anchor = if self.calendar_shows_today() {
            today
        } else {
            start
        };
        self.calendar_week.span = span;
        self.calendar_week.start = Some(self.calendar_span_start(anchor));
        if self.calendar_prefs.last_span != span {
            self.calendar_prefs.last_span = span;
            save_calendar_prefs(&self.store, &self.calendar_prefs);
        }
        if crate::call::calendar_now::shows_hour_grid(span) {
            self.request_calendar_scroll_to_now(cx);
        }
        self.load_calendar_week(cx);
    }

    pub fn show_calendar_date(&mut self, date: NaiveDate, cx: &mut Context<Self>) {
        self.calendar_week.start = Some(self.calendar_span_start(date));
        self.request_calendar_scroll_to_now(cx);
        self.load_calendar_week(cx);
    }

    pub fn show_calendar_day(&mut self, date: NaiveDate, cx: &mut Context<Self>) {
        self.calendar_week.span = CalendarSpan::Day;
        self.show_calendar_date(date, cx);
    }

    pub(crate) fn load_calendar_week(&mut self, cx: &mut Context<Self>) {
        let start = self.calendar_start();
        let offset = Local::now().offset().fix();
        let (from, to) = dates_range(&self.calendar_dates(), offset);
        if self.mode.demo {
            let mut events = demo_events(demo_anchor());
            events.extend(demo_week(week_start(start, Weekday::Mon), offset));
            events.extend(self.demo_meetings.iter().cloned());
            events.retain(|event| event.end > from && event.start < to);
            events.sort_by_key(|event| (event.start, event.end));
            self.calendar_week.events = events;
            cx.notify();
            return;
        }
        // Grid load needs only the signed-in session, never the calling engine.
        let Some(engine) = self.engine.clone() else {
            self.calendar_week.pending = true;
            return;
        };
        self.calendar_week.request += 1;
        let request = self.calendar_week.request;
        self.calendar_week.loading = true;
        cx.notify();
        let store = self.store.clone();
        let receiver = crate::runtime::spawn(async move {
            let started = Instant::now();
            let accounts = store::load_calendar_accounts(&store);
            let mut fetched_accounts = accounts.clone();
            let mut combined = Vec::new();
            let mut failures = Vec::new();
            let mut outlook_count = None;
            if accounts.outlook_selected {
                match Calendar::new(engine.remote().session())
                    .calendar_view(from, to)
                    .await
                {
                    Ok(events) => {
                        outlook_count = Some(events.len());
                        combined.extend(events);
                    }
                    Err(error) => failures.push(CalendarLoadFailure {
                        source: CalendarSource::Outlook,
                        message: error.to_string(),
                    }),
                }
            }
            let mut google_count = None;
            if accounts.has_google() {
                match crate::google_calendar_sync::fetch_google_events(
                    &mut fetched_accounts,
                    from,
                    to,
                )
                .await
                {
                    Ok(events) => {
                        // Keep only refreshed tokens: a toggle made meanwhile must survive.
                        let _ = store::merge_google_tokens(&store, &fetched_accounts);
                        google_count = Some(events.len());
                        combined.extend(events);
                    }
                    Err(error) => failures.push(CalendarLoadFailure {
                        source: CalendarSource::Google,
                        message: error,
                    }),
                }
            }
            crate::diagnostics::record(load_summary(
                from,
                to,
                outlook_count,
                google_count,
                started.elapsed().as_millis(),
                &failures,
            ));
            (chatsvc::merge_calendar_events(combined), failures)
        });
        cx.spawn(async move |this, cx| {
            let loaded = receiver.await.unwrap_or_else(|_| {
                (
                    Vec::new(),
                    vec![CalendarLoadFailure {
                        source: CalendarSource::Outlook,
                        message: CALENDAR_LOAD_FAILURE.to_owned(),
                    }],
                )
            });
            this.update(cx, |state, cx| {
                if state.calendar_week.request != request {
                    return;
                }
                state.calendar_week.loading = false;
                let (events, failures) = loaded;
                let previous = std::mem::take(&mut state.calendar_week.events);
                let mut events = keep_failed_sources(&previous, events, &failures);
                retain_peek_details(&previous, &mut events);
                state.calendar_week.events = events;
                state.calendar_week.failures = failures;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

/// A source that failed keeps its previous events on screen instead of blanking the week.
pub(crate) fn keep_failed_sources(
    previous: &[CalendarEvent],
    mut events: Vec<CalendarEvent>,
    failures: &[CalendarLoadFailure],
) -> Vec<CalendarEvent> {
    for failure in failures {
        events.extend(
            previous
                .iter()
                .filter(|event| event.source == failure.source)
                .cloned(),
        );
    }
    events.sort_by_key(|event| (event.start, event.end));
    events
}

/// One diagnostics line per grid load, so an empty week can be traced to its cause.
pub(crate) fn load_summary(
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
    outlook: Option<usize>,
    google: Option<usize>,
    elapsed_ms: u128,
    failures: &[CalendarLoadFailure],
) -> String {
    let count = |value: Option<usize>| value.map_or_else(|| "-".to_owned(), |n| n.to_string());
    let mut line = format!(
        "calendar grid: {}..{} outlook={} google={} in {elapsed_ms} ms",
        from.format("%Y-%m-%dT%H:%MZ"),
        to.format("%Y-%m-%dT%H:%MZ"),
        count(outlook),
        count(google),
    );
    for failure in failures {
        line.push_str(&format!(
            "; {} failed: {}",
            failure.source.label(),
            failure.message.chars().take(160).collect::<String>()
        ));
    }
    line
}

fn retain_peek_details(previous: &[CalendarEvent], events: &mut [CalendarEvent]) {
    for event in events {
        let Some(old) = previous
            .iter()
            .find(|candidate| candidate.object_id == event.object_id)
        else {
            continue;
        };
        if event.body_preview.is_none() {
            event.body_preview = old.body_preview.clone();
        }
        if event.body_html.is_none() {
            event.body_html = old.body_html.clone();
        }
        if event.attendees.is_empty() {
            event.attendees = old.attendees.clone();
        }
        if event.location.is_none() {
            event.location = old.location.clone();
        }
        if event.response_counts.is_none() {
            event.response_counts = old.response_counts;
        }
        if event.series_master_id.is_none() {
            event.series_master_id = old.series_master_id.clone();
        }
        if event.reminders.is_empty() {
            event.reminders = old.reminders.clone();
        }
        if event.source_link.is_none() {
            event.source_link = old.source_link.clone();
        }
        if event.web_link.is_none() {
            event.web_link = old.web_link.clone();
        }
        if event.calendar_name.is_none() {
            event.calendar_name = old.calendar_name.clone();
        }
        if old.series {
            event.series = true;
        }
    }
}

fn demo_show_as(subject: &str, response: ResponseType) -> ShowAs {
    match response {
        _ if subject.starts_with("Following: ") => ShowAs::Free,
        ResponseType::Tentative | ResponseType::NotResponded => ShowAs::Tentative,
        _ => ShowAs::Busy,
    }
}

fn demo_week(monday: NaiveDate, offset: FixedOffset) -> Vec<CalendarEvent> {
    let slots: [(&str, &str, i64, i64, i64, ResponseType); 8] = [
        (
            "Team standup",
            "Mara Lindqvist",
            0,
            9 * 60 + 30,
            15,
            ResponseType::Accepted,
        ),
        (
            "Release review",
            "Jonas Ortega",
            0,
            11 * 60,
            60,
            ResponseType::Organizer,
        ),
        (
            "Customer sync",
            "Priya Nair",
            0,
            11 * 60 + 30,
            60,
            ResponseType::NotResponded,
        ),
        (
            "Team standup",
            "Mara Lindqvist",
            1,
            9 * 60 + 30,
            15,
            ResponseType::Accepted,
        ),
        (
            "Architecture deep dive",
            "Lena Brandt",
            2,
            14 * 60,
            90,
            ResponseType::Tentative,
        ),
        (
            "1:1 Jonas",
            "Jonas Ortega",
            3,
            10 * 60,
            30,
            ResponseType::Accepted,
        ),
        (
            "Following: Design office hours",
            "Lena Brandt",
            3,
            13 * 60,
            60,
            ResponseType::Tentative,
        ),
        (
            "Sprint retro",
            "Mara Lindqvist",
            4,
            15 * 60,
            60,
            ResponseType::Accepted,
        ),
    ];
    slots
        .iter()
        .enumerate()
        .map(|(index, (subject, organizer, day, minute, length, response))| {
            let start =
                local_midnight(monday + Duration::days(*day), offset) + Duration::minutes(*minute);
            CalendarEvent {
                object_id: format!("demo-week-{index}"),
                subject: (*subject).to_owned(),
                start,
                end: start + Duration::minutes(*length),
                all_day: false,
                cancelled: false,
                online: true,
                join_url: Some(format!(
                    "https://teams.microsoft.com/l/meetup-join/19%3ameeting_demoweek{index}%40thread.v2/0"
                )),
                thread_id: None,
                organizer_name: Some((*organizer).to_owned()),
                organizer_address: None,
                ical_uid: None,
                show_as: demo_show_as(subject, *response),
                dial_in: None,
                response: *response,
                ..Default::default()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chatsvc::{CalendarEvent, CalendarSource};
    use chrono::{TimeZone, Utc};
    use gpui_kit::{AppContext as _, TestAppContext};
    use store::Store;

    use super::{CalendarLoadFailure, keep_failed_sources, load_summary};
    use crate::app_state::{AppState, Mode};

    fn event(id: &str, source: CalendarSource, hour: u32) -> CalendarEvent {
        let start = Utc.with_ymd_and_hms(2026, 11, 16, hour, 0, 0).unwrap();
        CalendarEvent {
            object_id: id.into(),
            subject: id.into(),
            start,
            end: start + chrono::Duration::minutes(30),
            source,
            ..Default::default()
        }
    }

    #[gpui_kit::test]
    fn a_load_before_the_session_runs_once_the_session_is_there(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let store = Arc::new(Store::open_in_memory().unwrap());
        let app = cx.update(|cx| cx.new(|_| AppState::new(store, Mode::default())));
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.open_calendar(cx);
                assert!(state.calendar_week.visible);
                assert!(state.calendar_week.take_pending());
                assert!(!state.calendar_week.take_pending());
            })
        });
    }

    #[gpui_kit::test]
    fn a_poll_reloads_the_grid_only_while_the_calendar_is_shown(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let store = Arc::new(Store::open_in_memory().unwrap());
        let app = cx.update(|cx| cx.new(|_| AppState::new(store, Mode::default())));
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.open_calendar(cx);
                state.calendar_week.take_pending();
                state.close_calendar();
                state.refresh_visible_calendar(cx);
                assert!(!state.calendar_week.take_pending());
                state.open_calendar(cx);
                state.calendar_week.take_pending();
                state.refresh_visible_calendar(cx);
                assert!(state.calendar_week.take_pending());
            })
        });
    }

    #[test]
    fn a_failed_source_keeps_its_previous_events() {
        let previous = vec![
            event("outlook-old", CalendarSource::Outlook, 9),
            event("google-old", CalendarSource::Google, 10),
        ];
        let fresh = vec![event("google-new", CalendarSource::Google, 8)];
        let failures = [CalendarLoadFailure {
            source: CalendarSource::Outlook,
            message: "HTTP 503".into(),
        }];
        let ids: Vec<_> = keep_failed_sources(&previous, fresh.clone(), &failures)
            .into_iter()
            .map(|event| event.object_id)
            .collect();
        assert_eq!(ids, ["google-new", "outlook-old"]);
        let ids: Vec<_> = keep_failed_sources(&previous, fresh, &[])
            .into_iter()
            .map(|event| event.object_id)
            .collect();
        assert_eq!(ids, ["google-new"]);
    }

    #[test]
    fn the_load_summary_names_counts_and_failures() {
        let from = Utc.with_ymd_and_hms(2026, 11, 15, 23, 0, 0).unwrap();
        let to = Utc.with_ymd_and_hms(2026, 11, 20, 23, 0, 0).unwrap();
        let failures = [CalendarLoadFailure {
            source: CalendarSource::Google,
            message: "client_secret is missing".into(),
        }];
        assert_eq!(
            load_summary(from, to, Some(3), None, 412, &failures),
            "calendar grid: 2026-11-15T23:00Z..2026-11-20T23:00Z outlook=3 google=- in 412 ms; \
             Google failed: client_secret is missing"
        );
    }
}
