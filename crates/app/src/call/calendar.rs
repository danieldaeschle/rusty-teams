use std::sync::OnceLock;

use chatsvc::CalendarEvent;
use chrono::{DateTime, Local, Offset, Utc};
use gpui_kit::Context;

use super::agenda::{AgendaRow, demo_events};
use crate::app_state::{AppEvent, AppState, Selection, selection_for};

const NO_MEETING_CHAT_NOTICE: &str = "This meeting chat is not available";
static DEMO_ANCHOR: OnceLock<DateTime<Utc>> = OnceLock::new();

impl AppState {
    pub fn apply_calendar(&mut self, events: Vec<CalendarEvent>, cx: &mut Context<Self>) {
        self.calendar_events = events;
        self.calls_list.invalidate();
        cx.emit(AppEvent::CallHistory);
        cx.notify();
    }

    pub fn refresh_calendar(&mut self, cx: &mut Context<Self>) {
        if self.mode.demo {
            let mut events = demo_events(*DEMO_ANCHOR.get_or_init(Utc::now));
            events.extend(self.demo_meetings.iter().cloned());
            events.sort_by_key(|event| (event.start, event.end));
            self.apply_calendar(events, cx);
            return;
        }
        if let Some(nudge) = &self.calendar_nudge {
            let _ = nudge.send(());
        }
    }

    pub fn rebuild_calls_list(&mut self, cx: &mut Context<Self>) {
        let now = Utc::now();
        let offset = Local::now().offset().fix();
        self.calls_list.rebuild(
            &self.call_history,
            &self.sidebar.chats,
            &self.calendar_events,
            now,
            offset,
        );
        cx.emit(AppEvent::CallHistory);
        cx.notify();
    }

    pub fn join_agenda_row(&mut self, row: &AgendaRow, cx: &mut Context<Self>) {
        if let Some(url) = &row.join_url {
            self.join_meeting_link(url, cx);
            if let Some(pending) = self.prejoin.as_mut() {
                pending.join.title = row.subject.clone();
            }
        }
    }

    pub fn open_agenda_chat(&mut self, row: &AgendaRow, cx: &mut Context<Self>) {
        let Some(thread_id) = &row.thread_id else {
            return;
        };
        match selection_for(&self.sidebar, thread_id) {
            Some(selection @ Selection::Chat(_)) => self.select(selection, cx),
            _ => self.raise_notice(NO_MEETING_CHAT_NOTICE.to_owned(), None, cx),
        }
    }
}
