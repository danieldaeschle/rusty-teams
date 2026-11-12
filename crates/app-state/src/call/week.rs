use chatsvc::{CalendarEvent, ResponseType, ShowAs};
use chrono::{Duration, FixedOffset, Local, NaiveDate, Offset};
use gpui_kit::Context;

use super::agenda::demo_events;
use super::calendar::demo_anchor;
use super::week_grid::{CalendarSpan, local_midnight, week_range, week_start};
use crate::app_state::AppState;

pub const CALENDAR_LOAD_FAILURE: &str = "We couldn't load your calendar.";

#[derive(Debug, Clone, Default)]
pub struct CalendarWeek {
    pub start: Option<NaiveDate>,
    pub span: CalendarSpan,
    pub events: Vec<CalendarEvent>,
    pub loading: bool,
    request: u64,
}

impl AppState {
    pub fn calendar_start(&self) -> NaiveDate {
        self.calendar_week
            .start
            .unwrap_or_else(|| self.calendar_week.span.start_for(Local::now().date_naive()))
    }

    pub fn open_calendar(&mut self, cx: &mut Context<Self>) {
        self.calendar_week.start = Some(self.calendar_start());
        self.request_calendar_scroll_to_now(cx);
        self.load_calendar_week(cx);
    }

    pub fn shift_calendar_week(&mut self, steps: i32, cx: &mut Context<Self>) {
        let span = self.calendar_week.span;
        self.calendar_week.start = Some(span.shifted(self.calendar_start(), steps));
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

    pub fn set_calendar_span(&mut self, span: CalendarSpan, cx: &mut Context<Self>) {
        let today = Local::now().date_naive();
        let start = self.calendar_start();
        let shown =
            start <= today && today < start + Duration::days(self.calendar_week.span.days());
        self.calendar_week.span = span;
        self.calendar_week.start = Some(span.start_for(if shown { today } else { start }));
        if crate::call::calendar_now::shows_hour_grid(span) {
            self.request_calendar_scroll_to_now(cx);
        }
        self.load_calendar_week(cx);
    }

    pub fn show_calendar_date(&mut self, date: NaiveDate, cx: &mut Context<Self>) {
        self.calendar_week.start = Some(self.calendar_week.span.start_for(date));
        self.load_calendar_week(cx);
    }

    pub fn show_calendar_day(&mut self, date: NaiveDate, cx: &mut Context<Self>) {
        self.calendar_week.span = CalendarSpan::Day;
        self.show_calendar_date(date, cx);
    }

    pub(crate) fn load_calendar_week(&mut self, cx: &mut Context<Self>) {
        let start = self.calendar_start();
        let offset = Local::now().offset().fix();
        let (from, to) = week_range(start, self.calendar_week.span.days(), offset);
        if self.mode.demo {
            let mut events = demo_events(demo_anchor());
            events.extend(demo_week(week_start(start), offset));
            events.extend(self.demo_meetings.iter().cloned());
            events.retain(|event| event.end > from && event.start < to);
            events.sort_by_key(|event| (event.start, event.end));
            self.calendar_week.events = events;
            cx.notify();
            return;
        }
        let Some(launcher) = self.call_launcher.clone() else {
            return;
        };
        self.calendar_week.request += 1;
        let request = self.calendar_week.request;
        self.calendar_week.loading = true;
        cx.notify();
        let store = self.store.clone();
        let receiver = crate::runtime::spawn(async move {
            let mut accounts = store::load_calendar_accounts(&store);
            let mut combined = Vec::new();
            let mut outlook_failed = false;
            if accounts.outlook_selected {
                match launcher.calendar_view(from, to).await {
                    Ok(events) => combined.extend(events),
                    Err(error) => {
                        outlook_failed = true;
                        crate::diagnostics::record(format!(
                            "calls: outlook calendar week not loaded: {error}"
                        ));
                    }
                }
            }
            match crate::google_calendar_sync::fetch_google_events(&mut accounts, from, to).await {
                Ok(events) => {
                    let _ = store::save_calendar_accounts(&store, &accounts);
                    combined.extend(events);
                }
                Err(error) => crate::diagnostics::record(format!(
                    "calls: google calendar week not loaded: {error}"
                )),
            }
            if combined.is_empty() && outlook_failed && !accounts.has_google() {
                return Err(CALENDAR_LOAD_FAILURE.to_owned());
            }
            Ok(chatsvc::merge_calendar_events(combined))
        });
        cx.spawn(async move |this, cx| {
            let loaded = receiver
                .await
                .unwrap_or_else(|_| Err(CALENDAR_LOAD_FAILURE.to_owned()));
            this.update(cx, |state, cx| {
                if state.calendar_week.request != request {
                    return;
                }
                state.calendar_week.loading = false;
                match loaded {
                    Ok(events) => state.calendar_week.events = events,
                    Err(notice) => state.raise_notice(notice, None, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
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
