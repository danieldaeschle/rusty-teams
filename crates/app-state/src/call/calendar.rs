use std::sync::OnceLock;

use chatsvc::CalendarEvent;
use chrono::{DateTime, Local, Offset, Utc};
use gpui_kit::Context;

use super::agenda::{AgendaRow, demo_events};
use super::list::HistoryFilter;
use crate::app_state::{AppEvent, AppState, Selection, selection_for};

const NO_MEETING_CHAT_NOTICE: &str = "This meeting chat is not available";
static DEMO_ANCHOR: OnceLock<DateTime<Utc>> = OnceLock::new();

pub(crate) fn demo_anchor() -> DateTime<Utc> {
    *DEMO_ANCHOR.get_or_init(Utc::now)
}

impl AppState {
    pub fn apply_calendar(&mut self, events: Vec<CalendarEvent>, cx: &mut Context<Self>) {
        // Agenda / reminders poll — must not replace calendar_week.events.
        // That window is only ~today+7d; overwriting wiped earlier days in the
        // visible week/month and made entries "vanish" after delete or poll.
        self.calendar_events = events;
        self.calls_list.invalidate();
        cx.emit(AppEvent::CallHistory);
        cx.notify();
    }

    pub fn refresh_calendar(&mut self, cx: &mut Context<Self>) {
        if self.mode.demo {
            let mut events = demo_events(demo_anchor());
            events.extend(self.demo_meetings.iter().cloned());
            events.sort_by_key(|event| (event.start, event.end));
            self.apply_calendar(events, cx);
            if self.calendar_week.start.is_some() {
                self.load_calendar_week(cx);
            }
            return;
        }
        if let Some(nudge) = &self.calendar_nudge {
            let _ = nudge.send(());
        }
        // Visible grid SoT is load_calendar_week (full span + Google merge).
        if self.calendar_week.start.is_some() {
            self.load_calendar_week(cx);
        }
    }

    pub fn rebuild_calls_list(&mut self, cx: &mut Context<Self>) {
        let now = Utc::now();
        let offset = Local::now().offset().fix();
        self.calls_list
            .rebuild(&self.call_history, &self.sidebar.chats, now, offset);
        cx.emit(AppEvent::CallHistory);
        cx.notify();
    }

    pub fn set_call_history_filter(&mut self, filter: HistoryFilter, cx: &mut Context<Self>) {
        self.calls_list.set_filter(filter);
        self.rebuild_calls_list(cx);
    }

    pub fn join_agenda_row(&mut self, row: &AgendaRow, cx: &mut Context<Self>) {
        self.join_calendar_meeting(row.join_url.as_deref(), &row.subject, cx);
    }

    pub fn join_calendar_meeting(
        &mut self,
        join_url: Option<&str>,
        subject: &str,
        cx: &mut Context<Self>,
    ) {
        if let Some(url) = join_url {
            self.join_meeting_link(url, cx);
            if let Some(pending) = self.prejoin.as_mut() {
                pending.join.title = subject.to_owned();
            }
        }
    }

    pub fn open_agenda_chat(&mut self, row: &AgendaRow, cx: &mut Context<Self>) {
        self.open_meeting_chat(row.thread_id.as_deref(), cx);
    }

    pub fn open_meeting_chat(&mut self, thread_id: Option<&str>, cx: &mut Context<Self>) {
        let Some(thread_id) = thread_id else {
            return;
        };
        match selection_for(&self.sidebar, thread_id) {
            Some(selection @ Selection::Chat(_)) => self.select(selection, cx),
            _ => self.raise_notice(NO_MEETING_CHAT_NOTICE.to_owned(), None, cx),
        }
    }
}
