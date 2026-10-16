use std::collections::HashSet;

use chatsvc::{CalendarEvent, ResponseType};
use chrono::{DateTime, Duration, FixedOffset, Utc};
use gpui_kit::Context;

use crate::app_state::{AppEvent, AppState};

pub const LEAD_MINUTES: i64 = 5;
pub const GRACE_MINUTES: i64 = 5;
const CHECK_EVERY_SECONDS: i64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReminderPhase {
    Soon,
    Started,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingToast {
    pub id: u64,
    pub event_key: String,
    pub phase: ReminderPhase,
    pub subject: String,
    pub start: DateTime<Utc>,
    pub join_url: String,
}

fn event_key(event: &CalendarEvent) -> String {
    format!("{}@{}", event.object_id, event.start.timestamp())
}

pub fn phase_at(event: &CalendarEvent, now: DateTime<Utc>) -> Option<ReminderPhase> {
    let eligible = event.online
        && !event.cancelled
        && !event.all_day
        && event.join_url.is_some()
        && event.response != ResponseType::Declined;
    if !eligible {
        return None;
    }
    if now >= event.start - Duration::minutes(LEAD_MINUTES) && now < event.start {
        return Some(ReminderPhase::Soon);
    }
    let started_until = event
        .end
        .min(event.start + Duration::minutes(GRACE_MINUTES));
    (now >= event.start && now < started_until).then_some(ReminderPhase::Started)
}

pub fn subtitle(toast: &MeetingToast, now: DateTime<Utc>, offset: FixedOffset) -> String {
    let clock = toast.start.with_timezone(&offset).format("%H:%M");
    match toast.phase {
        ReminderPhase::Started => format!("{clock} - started"),
        ReminderPhase::Soon => {
            let minutes = ((toast.start - now).num_seconds().max(0) + 59) / 60;
            match minutes {
                0 => format!("{clock} - starting now"),
                1 => format!("{clock} - starts in 1 min"),
                minutes => format!("{clock} - starts in {minutes} min"),
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct MeetingToasts {
    entries: Vec<MeetingToast>,
    fired: HashSet<(String, ReminderPhase)>,
    next_id: u64,
    last_check: Option<DateTime<Utc>>,
}

impl MeetingToasts {
    pub fn entries(&self) -> &[MeetingToast] {
        &self.entries
    }

    pub fn get(&self, id: u64) -> Option<&MeetingToast> {
        self.entries.iter().find(|toast| toast.id == id)
    }

    pub fn latest(&self) -> Option<&MeetingToast> {
        self.entries.last()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn dismiss(&mut self, id: u64) -> Option<MeetingToast> {
        let index = self.entries.iter().position(|toast| toast.id == id)?;
        Some(self.entries.remove(index))
    }

    pub fn clear(&mut self) -> bool {
        let had_any = !self.entries.is_empty();
        self.entries.clear();
        had_any
    }

    pub fn is_due_for_check(&self, now: DateTime<Utc>) -> bool {
        self.last_check
            .is_none_or(|last| now - last >= Duration::seconds(CHECK_EVERY_SECONDS))
    }

    pub fn update(&mut self, events: &[CalendarEvent], now: DateTime<Utc>, show: bool) -> bool {
        self.last_check = Some(now);
        let before = self.entries.clone();
        self.entries.retain(|toast| {
            events
                .iter()
                .any(|event| event_key(event) == toast.event_key && phase_at(event, now).is_some())
        });
        let keys: HashSet<String> = events.iter().map(event_key).collect();
        self.fired.retain(|(key, _)| keys.contains(key));
        for event in events {
            let Some(phase) = phase_at(event, now) else {
                continue;
            };
            let key = event_key(event);
            if !self.fired.insert((key.clone(), phase)) || !show {
                continue;
            }
            let Some(join_url) = event.join_url.clone() else {
                continue;
            };
            let subject = if event.subject.is_empty() {
                "(No title)".to_owned()
            } else {
                event.subject.clone()
            };
            if let Some(existing) = self.entries.iter_mut().find(|toast| toast.event_key == key) {
                existing.phase = phase;
                continue;
            }
            self.next_id += 1;
            self.entries.push(MeetingToast {
                id: self.next_id,
                event_key: key,
                phase,
                subject,
                start: event.start,
                join_url,
            });
        }
        self.entries != before
    }
}

impl AppState {
    pub fn tick_meeting_reminders(&mut self, show: bool, cx: &mut Context<Self>) {
        let now = Utc::now();
        let in_call = self.call.is_some();
        let show = show && !in_call && !self.mode.read_only;
        let changed = self.meeting_toasts.update(&self.calendar_events, now, show);
        if (in_call && self.meeting_toasts.clear()) || changed {
            cx.emit(AppEvent::MeetingReminder);
            cx.notify();
        }
    }

    pub fn join_meeting_toast(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(toast) = self.meeting_toasts.dismiss(id) else {
            return;
        };
        cx.emit(AppEvent::MeetingReminder);
        self.join_meeting_link(&toast.join_url, cx);
        if let Some(pending) = self.prejoin.as_mut() {
            pending.join.title = toast.subject;
        }
    }

    pub fn join_latest_meeting_toast(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.meeting_toasts.latest().map(|toast| toast.id) {
            self.join_meeting_toast(id, cx);
        }
    }

    pub fn dismiss_meeting_toast(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.meeting_toasts.dismiss(id).is_some() {
            cx.emit(AppEvent::MeetingReminder);
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn at(hour: u32, minute: u32, second: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 13, hour, minute, second)
            .unwrap()
    }

    fn event(id: &str) -> CalendarEvent {
        CalendarEvent {
            object_id: id.into(),
            subject: format!("Meeting {id}"),
            start: at(9, 0, 0),
            end: at(9, 30, 0),
            all_day: false,
            cancelled: false,
            online: true,
            join_url: Some("https://teams.microsoft.com/l/meetup-join/x".into()),
            thread_id: None,
            organizer_name: Some("Mara".into()),
            organizer_address: None,
            ical_uid: None,
            response: ResponseType::Accepted,
        }
    }

    #[test]
    fn the_phase_follows_the_clock_around_the_start() {
        let meeting = event("a");
        assert_eq!(phase_at(&meeting, at(8, 54, 59)), None);
        assert_eq!(phase_at(&meeting, at(8, 55, 0)), Some(ReminderPhase::Soon));
        assert_eq!(phase_at(&meeting, at(8, 59, 59)), Some(ReminderPhase::Soon));
        assert_eq!(
            phase_at(&meeting, at(9, 0, 0)),
            Some(ReminderPhase::Started)
        );
        assert_eq!(
            phase_at(&meeting, at(9, 4, 59)),
            Some(ReminderPhase::Started)
        );
        assert_eq!(phase_at(&meeting, at(9, 5, 0)), None);
    }

    #[test]
    fn a_short_meeting_stops_reminding_at_its_end() {
        let short = CalendarEvent {
            end: at(9, 3, 0),
            ..event("a")
        };
        assert_eq!(phase_at(&short, at(9, 2, 59)), Some(ReminderPhase::Started));
        assert_eq!(phase_at(&short, at(9, 3, 0)), None);
    }

    #[test]
    fn declined_cancelled_all_day_and_offline_events_stay_quiet() {
        let declined = CalendarEvent {
            response: ResponseType::Declined,
            ..event("a")
        };
        let cancelled = CalendarEvent {
            cancelled: true,
            ..event("a")
        };
        let all_day = CalendarEvent {
            all_day: true,
            ..event("a")
        };
        let offline = CalendarEvent {
            online: false,
            ..event("a")
        };
        for quiet in [declined, cancelled, all_day, offline] {
            assert_eq!(phase_at(&quiet, at(8, 58, 0)), None);
        }
    }

    #[test]
    fn each_phase_fires_once_per_meeting() {
        let events = [event("a")];
        let mut toasts = MeetingToasts::default();
        assert!(!toasts.update(&events, at(8, 54, 0), true));
        assert!(toasts.update(&events, at(8, 55, 0), true));
        assert_eq!(toasts.entries().len(), 1);
        let id = toasts.entries()[0].id;
        toasts.dismiss(id);
        assert!(!toasts.update(&events, at(8, 56, 0), true));
        assert!(toasts.is_empty());
    }

    #[test]
    fn the_start_brings_a_dismissed_toast_back_and_updates_an_open_one() {
        let events = [event("a")];
        let mut toasts = MeetingToasts::default();
        toasts.update(&events, at(8, 55, 0), true);
        let id = toasts.entries()[0].id;
        assert!(toasts.update(&events, at(9, 0, 0), true));
        assert_eq!(toasts.entries().len(), 1);
        assert_eq!(toasts.entries()[0].id, id);
        assert_eq!(toasts.entries()[0].phase, ReminderPhase::Started);
        let mut dismissed = MeetingToasts::default();
        dismissed.update(&events, at(8, 55, 0), true);
        let soon_id = dismissed.entries()[0].id;
        dismissed.dismiss(soon_id);
        assert!(dismissed.update(&events, at(9, 0, 0), true));
        assert_eq!(dismissed.entries()[0].phase, ReminderPhase::Started);
    }

    #[test]
    fn a_hidden_reminder_is_spent_and_does_not_appear_later() {
        let events = [event("a")];
        let mut toasts = MeetingToasts::default();
        assert!(!toasts.update(&events, at(8, 56, 0), false));
        assert!(!toasts.update(&events, at(8, 57, 0), true));
        assert!(toasts.is_empty());
    }

    #[test]
    fn toasts_leave_when_the_window_closes_or_the_event_goes() {
        let events = [event("a")];
        let mut toasts = MeetingToasts::default();
        toasts.update(&events, at(8, 56, 0), true);
        assert!(toasts.update(&events, at(9, 5, 0), true));
        assert!(toasts.is_empty());
        let mut removed = MeetingToasts::default();
        removed.update(&events, at(8, 56, 0), true);
        assert!(removed.update(&[], at(8, 57, 0), true));
        assert!(removed.is_empty());
    }

    #[test]
    fn the_latest_toast_is_the_one_a_shortcut_joins() {
        let events = [event("a"), event("b")];
        let mut toasts = MeetingToasts::default();
        toasts.update(&events, at(8, 56, 0), true);
        assert_eq!(
            toasts.latest().map(|toast| toast.subject.as_str()),
            Some("Meeting b")
        );
    }

    #[test]
    fn the_subtitle_counts_down_to_the_start_in_local_time() {
        let toast = |phase| MeetingToast {
            id: 1,
            event_key: "a".into(),
            phase,
            subject: "Planning".into(),
            start: at(9, 0, 0),
            join_url: "https://example.invalid".into(),
        };
        let offset = FixedOffset::east_opt(2 * 3600).unwrap();
        let soon = toast(ReminderPhase::Soon);
        assert_eq!(
            subtitle(&soon, at(8, 55, 0), offset),
            "11:00 - starts in 5 min"
        );
        assert_eq!(
            subtitle(&soon, at(8, 58, 30), offset),
            "11:00 - starts in 2 min"
        );
        assert_eq!(
            subtitle(&soon, at(8, 59, 30), offset),
            "11:00 - starts in 1 min"
        );
        assert_eq!(subtitle(&soon, at(9, 0, 0), offset), "11:00 - starting now");
        assert_eq!(
            subtitle(&toast(ReminderPhase::Started), at(9, 1, 0), offset),
            "11:00 - started"
        );
    }

    #[test]
    fn clearing_removes_every_open_toast() {
        let events = [event("a"), event("b")];
        let mut toasts = MeetingToasts::default();
        toasts.update(&events, at(8, 56, 0), true);
        assert!(toasts.clear());
        assert!(toasts.is_empty());
        assert!(!toasts.clear());
    }

    #[test]
    fn checks_run_at_most_once_a_second() {
        let mut toasts = MeetingToasts::default();
        assert!(toasts.is_due_for_check(at(9, 0, 0)));
        toasts.update(&[], at(9, 0, 0), true);
        assert!(!toasts.is_due_for_check(at(9, 0, 0)));
        assert!(toasts.is_due_for_check(at(9, 0, 1)));
    }
}
