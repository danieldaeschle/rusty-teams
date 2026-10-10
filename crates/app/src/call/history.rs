use std::collections::HashMap;

use chrono::{DateTime, Duration, FixedOffset, Utc};
use gpui_kit::Context;
use store::ChatRecord;
use teams_core::{CallDirection, CallLogEntry, CallOutcome};

use crate::app_state::{AppEvent, AppState, Selection, chat_title};
use crate::data::is_one_on_one;
use crate::format;
use crate::notice::short_error;
use crate::runtime;

const ORGID_PREFIX: &str = "8:orgid:";
const UNKNOWN_NAME: &str = "Unknown";
const MEETING_TITLE: &str = "Meeting";
const NO_CHAT_NOTICE: &str = "No chat with this person yet";
const NO_MEETING_CHAT_NOTICE: &str = "This meeting chat is not available";
const REFRESH_DELAY_AFTER_CALL: std::time::Duration = std::time::Duration::from_secs(4);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallRow {
    pub key: String,
    pub title: String,
    pub subline: String,
    pub time_label: String,
    pub missed: bool,
    pub meeting: bool,
    pub user_id: Option<String>,
    pub thread_id: Option<String>,
}

pub fn duration_label(seconds: i64) -> String {
    let seconds = seconds.max(0);
    let (hours, minutes, rest) = (seconds / 3600, seconds % 3600 / 60, seconds % 60);
    match (hours, minutes, rest) {
        (0, 0, _) => format!("{rest}s"),
        (0, minutes, 0) => format!("{minutes}m"),
        (0, minutes, rest) if minutes < 10 => format!("{minutes}m {rest}s"),
        (0, minutes, _) => format!("{minutes}m"),
        (hours, 0, _) => format!("{hours}h"),
        (hours, minutes, _) => format!("{hours}h {minutes}m"),
    }
}

pub fn subline(entry: &CallLogEntry) -> String {
    if entry.missed() {
        return "Missed call".to_owned();
    }
    let kind = if entry.is_meeting {
        "Meeting"
    } else {
        match (entry.direction, entry.outcome) {
            (CallDirection::Incoming, CallOutcome::Declined) => return "Declined call".to_owned(),
            (CallDirection::Incoming, _) => "Incoming",
            (CallDirection::Outgoing, _) => "Outgoing",
        }
    };
    match entry.duration_seconds {
        Some(seconds) => format!("{kind} \u{b7} {}", duration_label(seconds)),
        None => kind.to_owned(),
    }
}

fn user_id_of(mri: &str) -> Option<String> {
    mri.strip_prefix(ORGID_PREFIX).map(str::to_owned)
}

fn known_names(chats: &[ChatRecord]) -> HashMap<&str, &str> {
    chats
        .iter()
        .flat_map(|chat| chat.members.iter())
        .filter_map(|member| Some((member.user_id.as_deref()?, member.display_name.as_str())))
        .filter(|(_, name)| !name.is_empty())
        .collect()
}

pub fn call_rows(
    entries: &[CallLogEntry],
    chats: &[ChatRecord],
    now: DateTime<Utc>,
    offset: FixedOffset,
) -> Vec<CallRow> {
    let names = known_names(chats);
    let today = now.with_timezone(&offset).date_naive();
    entries
        .iter()
        .map(|entry| {
            let user_id = entry
                .peer_id
                .as_deref()
                .and_then(user_id_of)
                .filter(|_| !entry.is_meeting);
            let thread_chat = entry
                .thread_id
                .as_deref()
                .and_then(|thread_id| chats.iter().find(|chat| chat.id == thread_id));
            let title = if entry.is_meeting {
                thread_chat.map_or_else(|| MEETING_TITLE.to_owned(), chat_title)
            } else {
                entry
                    .peer_name
                    .clone()
                    .or_else(|| {
                        let user_id = user_id.as_deref()?;
                        names.get(user_id).map(|name| (*name).to_owned())
                    })
                    .unwrap_or_else(|| UNKNOWN_NAME.to_owned())
            };
            CallRow {
                key: entry.call_id.clone(),
                title,
                subline: subline(entry),
                time_label: format::list_time_label(entry.started_at, today, offset),
                missed: entry.missed(),
                meeting: entry.is_meeting,
                user_id,
                thread_id: entry.thread_id.clone().filter(|_| entry.is_meeting),
            }
        })
        .collect()
}

pub fn demo_call_log(now: DateTime<Utc>) -> Vec<CallLogEntry> {
    let entry = |id: &str,
                 minutes_ago: i64,
                 direction: CallDirection,
                 outcome: CallOutcome,
                 duration_seconds: Option<i64>,
                 peer: Option<(&str, &str)>,
                 thread_id: Option<&str>| CallLogEntry {
        call_id: id.to_owned(),
        started_at: now - Duration::minutes(minutes_ago),
        duration_seconds,
        direction,
        outcome,
        is_meeting: thread_id.is_some(),
        peer_id: peer.map(|(user_id, _)| format!("{ORGID_PREFIX}{user_id}")),
        peer_name: peer.map(|(_, name)| name.to_owned()),
        thread_id: thread_id.map(str::to_owned),
    };
    let incoming = CallDirection::Incoming;
    let outgoing = CallDirection::Outgoing;
    let accepted = CallOutcome::Accepted;
    let mara = Some(("demo-mara", "Mara Lindqvist"));
    let jonas = Some(("demo-jonas", "Jonas Ortega"));
    let priya = Some(("demo-priya", "Priya Nair"));
    vec![
        entry(
            "demo-call-1",
            35,
            incoming,
            CallOutcome::Missed,
            None,
            mara,
            None,
        ),
        entry(
            "demo-call-2",
            190,
            outgoing,
            accepted,
            Some(252),
            jonas,
            None,
        ),
        entry(
            "demo-call-3",
            1500,
            incoming,
            accepted,
            Some(720),
            priya,
            None,
        ),
        entry(
            "demo-call-4",
            1620,
            incoming,
            accepted,
            Some(2280),
            None,
            Some("demo-chat-meeting-standup"),
        ),
        entry(
            "demo-call-5",
            4400,
            incoming,
            CallOutcome::Missed,
            None,
            jonas,
            None,
        ),
        entry(
            "demo-call-6",
            8200,
            outgoing,
            accepted,
            Some(95),
            mara,
            None,
        ),
        entry(
            "demo-call-7",
            11000,
            incoming,
            accepted,
            Some(3900),
            None,
            Some("demo-chat-meeting-retro"),
        ),
    ]
}

impl AppState {
    pub fn refresh_call_history(&mut self, cx: &mut Context<Self>) {
        if self.mode.demo {
            self.call_history = demo_call_log(Utc::now());
            cx.emit(AppEvent::CallHistory);
            cx.notify();
            return;
        }
        let Some(engine) = self.engine.clone() else {
            return;
        };
        let receiver = runtime::spawn(async move { engine.list_call_logs().await });
        cx.spawn(async move |this, cx| match receiver.await {
            Ok(Ok(entries)) => {
                this.update(cx, |state, cx| {
                    state.call_history = entries;
                    cx.emit(AppEvent::CallHistory);
                    cx.notify();
                })
                .ok();
            }
            Ok(Err(error)) => eprintln!("calls: history not loaded: {}", short_error(&error)),
            Err(_) => {}
        })
        .detach();
    }

    pub fn refresh_call_history_after_call(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(REFRESH_DELAY_AFTER_CALL)
                .await;
            this.update(cx, |state, cx| state.refresh_call_history(cx))
                .ok();
        })
        .detach();
    }

    fn one_on_one_chat_with(&self, user_id: &str) -> Option<String> {
        self.sidebar
            .chats
            .iter()
            .find(|chat| {
                is_one_on_one(chat)
                    && chat
                        .members
                        .iter()
                        .any(|member| member.user_id.as_deref() == Some(user_id))
            })
            .map(|chat| chat.id.clone())
    }

    fn chat_of_call_row(&self, row: &CallRow) -> Option<String> {
        match (&row.thread_id, &row.user_id) {
            (Some(thread_id), _) => self
                .sidebar
                .chats
                .iter()
                .any(|chat| &chat.id == thread_id)
                .then(|| thread_id.clone()),
            (None, Some(user_id)) => self.one_on_one_chat_with(user_id),
            (None, None) => None,
        }
    }

    pub fn call_back_from_history(&mut self, row: &CallRow, cx: &mut Context<Self>) {
        let Some(user_id) = row.user_id.as_deref() else {
            return;
        };
        match self.one_on_one_chat_with(user_id) {
            Some(chat_id) => self.start_chat_call(&chat_id, cx),
            None => self.raise_notice(NO_CHAT_NOTICE.to_owned(), None, cx),
        }
    }

    pub fn open_chat_of_call_row(&mut self, row: &CallRow, cx: &mut Context<Self>) {
        let notice = if row.meeting {
            NO_MEETING_CHAT_NOTICE
        } else {
            NO_CHAT_NOTICE
        };
        match self.chat_of_call_row(row) {
            Some(chat_id) => self.select(Selection::Chat(chat_id), cx),
            None => self.raise_notice(notice.to_owned(), None, cx),
        }
    }

    pub fn activate_call_row(&mut self, row: &CallRow, cx: &mut Context<Self>) {
        if row.meeting {
            self.open_chat_of_call_row(row, cx);
        } else {
            self.call_back_from_history(row, cx);
        }
    }

    pub fn can_open_chat_of_call_row(&self, row: &CallRow) -> bool {
        self.chat_of_call_row(row).is_some()
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use store::MemberRecord;

    use super::*;

    fn entry(direction: CallDirection, outcome: CallOutcome, seconds: Option<i64>) -> CallLogEntry {
        CallLogEntry {
            call_id: "c".into(),
            started_at: Utc.with_ymd_and_hms(2026, 10, 9, 8, 0, 0).unwrap(),
            duration_seconds: seconds,
            direction,
            outcome,
            is_meeting: false,
            peer_id: Some("8:orgid:bea-id".into()),
            peer_name: None,
            thread_id: None,
        }
    }

    fn chat(id: &str, kind: &str, title: &str, members: &[(&str, &str)]) -> ChatRecord {
        ChatRecord {
            id: id.into(),
            kind: kind.into(),
            title: title.into(),
            members: members
                .iter()
                .map(|(user_id, name)| MemberRecord {
                    user_id: Some((*user_id).to_owned()),
                    display_name: (*name).to_owned(),
                })
                .collect(),
            ..Default::default()
        }
    }

    fn rows(entries: &[CallLogEntry], chats: &[ChatRecord]) -> Vec<CallRow> {
        let now = Utc.with_ymd_and_hms(2026, 10, 9, 12, 0, 0).unwrap();
        call_rows(entries, chats, now, FixedOffset::east_opt(0).unwrap())
    }

    #[test]
    fn durations_show_seconds_only_for_short_calls() {
        assert_eq!(duration_label(45), "45s");
        assert_eq!(duration_label(252), "4m 12s");
        assert_eq!(duration_label(240), "4m");
        assert_eq!(duration_label(720), "12m");
        assert_eq!(duration_label(2280), "38m");
        assert_eq!(duration_label(3600), "1h");
        assert_eq!(duration_label(3900), "1h 5m");
    }

    #[test]
    fn sublines_follow_direction_and_outcome() {
        let missed = entry(CallDirection::Incoming, CallOutcome::Missed, None);
        assert_eq!(subline(&missed), "Missed call");
        let outgoing = entry(CallDirection::Outgoing, CallOutcome::Accepted, Some(252));
        assert_eq!(subline(&outgoing), "Outgoing \u{b7} 4m 12s");
        let incoming = entry(CallDirection::Incoming, CallOutcome::Accepted, Some(720));
        assert_eq!(subline(&incoming), "Incoming \u{b7} 12m");
        let declined = entry(CallDirection::Incoming, CallOutcome::Declined, None);
        assert_eq!(subline(&declined), "Declined call");
        let unanswered = entry(CallDirection::Outgoing, CallOutcome::Missed, None);
        assert_eq!(subline(&unanswered), "Outgoing");
        let meeting = CallLogEntry {
            is_meeting: true,
            ..entry(CallDirection::Incoming, CallOutcome::Accepted, Some(2280))
        };
        assert_eq!(subline(&meeting), "Meeting \u{b7} 38m");
    }

    #[test]
    fn a_missing_display_name_comes_from_the_chat_members() {
        let chats = [chat("19:a", "oneOnOne", "", &[("bea-id", "Bea")])];
        let list = rows(
            &[entry(
                CallDirection::Outgoing,
                CallOutcome::Accepted,
                Some(60),
            )],
            &chats,
        );
        assert_eq!(list[0].title, "Bea");
        assert_eq!(list[0].user_id.as_deref(), Some("bea-id"));
        let unknown = rows(
            &[entry(
                CallDirection::Outgoing,
                CallOutcome::Accepted,
                Some(60),
            )],
            &[],
        );
        assert_eq!(unknown[0].title, "Unknown");
    }

    #[test]
    fn a_meeting_takes_the_meeting_chat_title() {
        let chats = [chat("19:meeting_x", "meeting", "Sprint retro", &[])];
        let meeting = CallLogEntry {
            is_meeting: true,
            thread_id: Some("19:meeting_x".into()),
            ..entry(CallDirection::Incoming, CallOutcome::Accepted, Some(60))
        };
        let list = rows(std::slice::from_ref(&meeting), &chats);
        assert_eq!(list[0].title, "Sprint retro");
        assert_eq!(list[0].thread_id.as_deref(), Some("19:meeting_x"));
        assert_eq!(list[0].user_id, None);
        assert_eq!(rows(&[meeting], &[])[0].title, "Meeting");
    }

    #[test]
    fn rows_carry_the_chat_list_time_label_and_the_missed_flag() {
        let missed = entry(CallDirection::Incoming, CallOutcome::Missed, None);
        let list = rows(&[missed], &[]);
        assert_eq!(list[0].time_label, "08:00");
        assert!(list[0].missed);
    }

    #[test]
    fn the_demo_history_has_every_kind_of_row() {
        let log = demo_call_log(Utc::now());
        assert!(log.iter().any(CallLogEntry::missed));
        assert!(
            log.iter()
                .any(|entry| entry.direction == CallDirection::Outgoing)
        );
        assert!(log.iter().any(|entry| entry.is_meeting));
        assert!(
            log.iter()
                .any(|entry| entry.direction == CallDirection::Incoming
                    && entry.outcome == CallOutcome::Accepted
                    && !entry.is_meeting)
        );
    }
}
