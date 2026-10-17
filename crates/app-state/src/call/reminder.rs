use std::collections::HashSet;

use chatsvc::{CalendarEvent, ResponseType};
use chrono::{DateTime, Duration, FixedOffset, Utc};
use gpui_kit::Context;

use super::outlook_reminders::{OutlookReminders, ReminderWrite};
use crate::app_state::{AppEvent, AppState};

pub const LEAD_MINUTES: i64 = 5;
pub const GRACE_MINUTES: i64 = 5;
const CHECK_EVERY_SECONDS: i64 = 1;
const LIVE_POLL_SECONDS: i64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReminderPhase {
    Soon,
    Started,
    Live,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingToast {
    pub id: u64,
    pub event_key: String,
    pub event_id: String,
    pub phase: ReminderPhase,
    pub subject: String,
    pub start: DateTime<Utc>,
    pub join_url: String,
}

fn event_key(event: &CalendarEvent) -> String {
    format!("{}@{}", event.object_id, event.start.timestamp())
}

pub(super) fn is_eligible(event: &CalendarEvent) -> bool {
    event.online
        && !event.cancelled
        && !event.all_day
        && event.join_url.is_some()
        && event.response != ResponseType::Declined
}

pub fn phase_at(event: &CalendarEvent, now: DateTime<Utc>, live: bool) -> Option<ReminderPhase> {
    if !is_eligible(event) {
        return None;
    }
    let opens = event.start - Duration::minutes(LEAD_MINUTES);
    let started_until = event
        .end
        .min(event.start + Duration::minutes(GRACE_MINUTES));
    if live && now >= opens && now < started_until {
        return Some(ReminderPhase::Live);
    }
    if now >= opens && now < event.start {
        return Some(ReminderPhase::Soon);
    }
    (now >= event.start && now < started_until).then_some(ReminderPhase::Started)
}

fn is_live(event: &CalendarEvent, live_threads: &HashSet<String>) -> bool {
    event
        .thread_id
        .as_ref()
        .is_some_and(|thread_id| live_threads.contains(thread_id))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReminderUpdate {
    pub changed: bool,
    pub chime: bool,
}

pub fn subtitle(toast: &MeetingToast, now: DateTime<Utc>, offset: FixedOffset) -> String {
    let clock = toast.start.with_timezone(&offset).format("%H:%M");
    match toast.phase {
        ReminderPhase::Started => format!("{clock} - started"),
        ReminderPhase::Live => format!("{clock} - live now"),
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
    snoozed: HashSet<String>,
    dismissed: HashSet<String>,
    chimed: HashSet<String>,
    next_id: u64,
    last_check: Option<DateTime<Utc>>,
    last_live_poll: Option<DateTime<Utc>>,
    outlook: OutlookReminders,
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

    pub fn outlook_mut(&mut self) -> &mut OutlookReminders {
        &mut self.outlook
    }

    pub fn dismiss(&mut self, id: u64) -> Option<MeetingToast> {
        let index = self.entries.iter().position(|toast| toast.id == id)?;
        let toast = self.entries.remove(index);
        self.dismissed.insert(toast.event_key.clone());
        Some(toast)
    }

    pub fn snooze(&mut self, id: u64) -> Option<MeetingToast> {
        let index = self.entries.iter().position(|toast| toast.id == id)?;
        let toast = self.entries.remove(index);
        self.snoozed.insert(toast.event_key.clone());
        Some(toast)
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

    pub fn live_candidates(&mut self, events: &[CalendarEvent], now: DateTime<Utc>) -> Vec<String> {
        let due = self
            .last_live_poll
            .is_none_or(|last| now - last >= Duration::seconds(LIVE_POLL_SECONDS));
        if !due {
            return Vec::new();
        }
        let candidates: Vec<String> = events
            .iter()
            .filter(|event| {
                phase_at(event, now, false).is_some() && !self.dismissed.contains(&event_key(event))
            })
            .filter_map(|event| event.thread_id.clone())
            .collect();
        if !candidates.is_empty() {
            self.last_live_poll = Some(now);
        }
        candidates
    }

    pub fn update(
        &mut self,
        events: &[CalendarEvent],
        live_threads: &HashSet<String>,
        now: DateTime<Utc>,
        show: bool,
    ) -> ReminderUpdate {
        self.last_check = Some(now);
        let before = self.entries.clone();
        let outlook = &self.outlook;
        self.entries.retain(|toast| {
            events.iter().any(|event| {
                event_key(event) == toast.event_key
                    && outlook.allows(event, now)
                    && phase_at(event, now, is_live(event, live_threads)).is_some()
            })
        });
        let keys: HashSet<String> = events.iter().map(event_key).collect();
        self.fired.retain(|(key, _)| keys.contains(key));
        self.snoozed.retain(|key| keys.contains(key));
        self.dismissed.retain(|key| keys.contains(key));
        self.chimed.retain(|key| keys.contains(key));
        let mut chime = false;
        for event in events {
            let Some(phase) = phase_at(event, now, is_live(event, live_threads)) else {
                continue;
            };
            let key = event_key(event);
            if self.dismissed.contains(&key) {
                continue;
            }
            if !self.outlook.allows(event, now) {
                self.fired.retain(|(fired_key, _)| *fired_key != key);
                continue;
            }
            let chime_due = phase != ReminderPhase::Soon && !self.chimed.contains(&key);
            if let Some(existing) = self.entries.iter_mut().find(|toast| toast.event_key == key) {
                existing.phase = phase;
                if chime_due {
                    self.chimed.insert(key);
                    chime |= show;
                }
                continue;
            }
            if self.snoozed.contains(&key) {
                if phase == ReminderPhase::Soon {
                    continue;
                }
                self.snoozed.remove(&key);
            } else if self.fired.contains(&(key.clone(), phase)) {
                continue;
            }
            self.fired.insert((key.clone(), phase));
            if !show {
                self.chimed.insert(key);
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
            if chime_due {
                self.chimed.insert(key.clone());
                chime = true;
            }
            self.next_id += 1;
            self.entries.push(MeetingToast {
                id: self.next_id,
                event_key: key,
                event_id: event.object_id.clone(),
                phase,
                subject,
                start: event.start,
                join_url,
            });
        }
        ReminderUpdate {
            changed: self.entries != before,
            chime,
        }
    }
}

impl AppState {
    pub fn tick_meeting_reminders(&mut self, show: bool, cx: &mut Context<Self>) {
        let now = Utc::now();
        self.poll_outlook_reminders(now, cx);
        let in_call = self.call.is_some();
        let show = show && !in_call && !self.mode.read_only;
        for thread_id in self
            .meeting_toasts
            .live_candidates(&self.calendar_events, now)
        {
            self.fetch_live_meeting(&thread_id, cx);
        }
        let live_threads: HashSet<String> = self
            .calendar_events
            .iter()
            .filter_map(|event| event.thread_id.clone())
            .filter(|thread_id| self.running_meeting(thread_id).is_some())
            .collect();
        let update = self
            .meeting_toasts
            .update(&self.calendar_events, &live_threads, now, show);
        if (in_call && self.meeting_toasts.clear()) || update.changed {
            cx.emit(AppEvent::MeetingReminder);
            cx.notify();
        }
        if update.chime {
            cx.emit(AppEvent::MeetingChime);
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

    pub fn snooze_meeting_toast(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(toast) = self.meeting_toasts.snooze(id) {
            self.write_outlook_reminder(toast.event_id, ReminderWrite::Snooze(toast.start));
            cx.emit(AppEvent::MeetingReminder);
            cx.notify();
        }
    }

    pub fn dismiss_meeting_toast(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(toast) = self.meeting_toasts.dismiss(id) {
            self.write_outlook_reminder(toast.event_id, ReminderWrite::Dismiss);
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

    fn tick(
        toasts: &mut MeetingToasts,
        events: &[CalendarEvent],
        now: DateTime<Utc>,
        show: bool,
    ) -> ReminderUpdate {
        toasts.update(events, &HashSet::new(), now, show)
    }

    fn tick_live(
        toasts: &mut MeetingToasts,
        events: &[CalendarEvent],
        now: DateTime<Utc>,
    ) -> ReminderUpdate {
        let live_threads = HashSet::from(["thread".to_owned()]);
        toasts.update(events, &live_threads, now, true)
    }

    fn live_event(id: &str) -> CalendarEvent {
        CalendarEvent {
            thread_id: Some("thread".into()),
            ..event(id)
        }
    }

    #[test]
    fn the_phase_follows_the_clock_around_the_start() {
        let meeting = event("a");
        assert_eq!(phase_at(&meeting, at(8, 54, 59), false), None);
        assert_eq!(
            phase_at(&meeting, at(8, 55, 0), false),
            Some(ReminderPhase::Soon)
        );
        assert_eq!(
            phase_at(&meeting, at(8, 59, 59), false),
            Some(ReminderPhase::Soon)
        );
        assert_eq!(
            phase_at(&meeting, at(9, 0, 0), false),
            Some(ReminderPhase::Started)
        );
        assert_eq!(
            phase_at(&meeting, at(9, 4, 59), false),
            Some(ReminderPhase::Started)
        );
        assert_eq!(phase_at(&meeting, at(9, 5, 0), false), None);
    }

    #[test]
    fn a_short_meeting_stops_reminding_at_its_end() {
        let short = CalendarEvent {
            end: at(9, 3, 0),
            ..event("a")
        };
        assert_eq!(
            phase_at(&short, at(9, 2, 59), false),
            Some(ReminderPhase::Started)
        );
        assert_eq!(phase_at(&short, at(9, 3, 0), false), None);
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
            assert_eq!(phase_at(&quiet, at(8, 58, 0), false), None);
        }
    }

    #[test]
    fn each_phase_fires_once_per_meeting() {
        let events = [event("a")];
        let mut toasts = MeetingToasts::default();
        assert!(!tick(&mut toasts, &events, at(8, 54, 0), true).changed);
        assert!(tick(&mut toasts, &events, at(8, 55, 0), true).changed);
        assert_eq!(toasts.entries().len(), 1);
        let id = toasts.entries()[0].id;
        toasts.dismiss(id);
        assert!(!tick(&mut toasts, &events, at(8, 56, 0), true).changed);
        assert!(toasts.is_empty());
    }

    #[test]
    fn the_start_moves_an_open_toast_to_started_and_chimes() {
        let events = [event("a")];
        let mut toasts = MeetingToasts::default();
        assert!(!tick(&mut toasts, &events, at(8, 55, 0), true).chime);
        let id = toasts.entries()[0].id;
        let update = tick(&mut toasts, &events, at(9, 0, 0), true);
        assert!(update.changed && update.chime);
        assert_eq!(toasts.entries().len(), 1);
        assert_eq!(toasts.entries()[0].id, id);
        assert_eq!(toasts.entries()[0].phase, ReminderPhase::Started);
        assert!(!tick(&mut toasts, &events, at(9, 1, 0), true).chime);
    }

    #[test]
    fn a_toast_closed_with_the_corner_x_stays_gone_and_silent() {
        let events = [event("a")];
        let mut toasts = MeetingToasts::default();
        tick(&mut toasts, &events, at(8, 55, 0), true);
        let id = toasts.entries()[0].id;
        toasts.dismiss(id);
        let update = tick(&mut toasts, &events, at(9, 0, 0), true);
        assert!(!update.changed && !update.chime);
        assert!(toasts.is_empty());
    }

    #[test]
    fn the_corner_x_only_closes_its_own_meeting() {
        let events = [event("a"), event("b")];
        let mut toasts = MeetingToasts::default();
        tick(&mut toasts, &events, at(8, 55, 0), true);
        let first = toasts.entries()[0].id;
        toasts.dismiss(first);
        let update = tick(&mut toasts, &events, at(9, 0, 0), true);
        assert!(update.chime);
        assert_eq!(toasts.entries().len(), 1);
        assert_eq!(toasts.entries()[0].subject, "Meeting b");
        assert_eq!(toasts.entries()[0].phase, ReminderPhase::Started);
    }

    #[test]
    fn a_snoozed_toast_comes_back_at_the_start_with_a_chime() {
        let events = [event("a")];
        let mut toasts = MeetingToasts::default();
        tick(&mut toasts, &events, at(8, 55, 0), true);
        let id = toasts.entries()[0].id;
        toasts.snooze(id);
        let quiet = tick(&mut toasts, &events, at(8, 58, 0), true);
        assert!(!quiet.changed && !quiet.chime);
        let update = tick(&mut toasts, &events, at(9, 0, 0), true);
        assert!(update.changed && update.chime);
        assert_eq!(toasts.entries()[0].phase, ReminderPhase::Started);
    }

    #[test]
    fn a_snoozed_toast_comes_back_live_before_the_start_with_a_chime() {
        let events = [live_event("a")];
        let mut toasts = MeetingToasts::default();
        tick(&mut toasts, &events, at(8, 55, 0), true);
        let id = toasts.entries()[0].id;
        toasts.snooze(id);
        let update = tick_live(&mut toasts, &events, at(8, 57, 0));
        assert!(update.changed && update.chime);
        assert_eq!(toasts.entries()[0].phase, ReminderPhase::Live);
    }

    #[test]
    fn a_toast_closed_with_the_corner_x_stays_gone_when_the_meeting_goes_live() {
        let events = [live_event("a")];
        let mut toasts = MeetingToasts::default();
        tick(&mut toasts, &events, at(8, 55, 0), true);
        let id = toasts.entries()[0].id;
        toasts.dismiss(id);
        let update = tick_live(&mut toasts, &events, at(8, 57, 0));
        assert!(!update.changed && !update.chime);
        assert!(toasts.is_empty());
    }

    #[test]
    fn started_then_live_chimes_once() {
        let events = [live_event("a")];
        let mut toasts = MeetingToasts::default();
        assert!(tick(&mut toasts, &events, at(9, 0, 0), true).chime);
        let update = tick_live(&mut toasts, &events, at(9, 1, 0));
        assert!(update.changed && !update.chime);
        assert_eq!(toasts.entries()[0].phase, ReminderPhase::Live);
        assert!(!tick(&mut toasts, &events, at(9, 2, 0), true).chime);
    }

    #[test]
    fn live_then_started_chimes_once() {
        let events = [live_event("a")];
        let mut toasts = MeetingToasts::default();
        assert!(tick_live(&mut toasts, &events, at(8, 58, 0)).chime);
        assert!(!tick_live(&mut toasts, &events, at(9, 0, 0)).chime);
        let update = tick(&mut toasts, &events, at(9, 1, 0), true);
        assert!(!update.chime);
        assert_eq!(toasts.entries()[0].phase, ReminderPhase::Started);
    }

    #[test]
    fn the_live_phase_covers_the_window_and_wins_over_soon_and_started() {
        let meeting = event("a");
        assert_eq!(phase_at(&meeting, at(8, 54, 59), true), None);
        assert_eq!(
            phase_at(&meeting, at(8, 55, 0), true),
            Some(ReminderPhase::Live)
        );
        assert_eq!(
            phase_at(&meeting, at(9, 0, 0), true),
            Some(ReminderPhase::Live)
        );
        assert_eq!(
            phase_at(&meeting, at(9, 4, 59), true),
            Some(ReminderPhase::Live)
        );
        assert_eq!(phase_at(&meeting, at(9, 5, 0), true), None);
        let declined = CalendarEvent {
            response: ResponseType::Declined,
            ..event("a")
        };
        assert_eq!(phase_at(&declined, at(9, 0, 0), true), None);
    }

    #[test]
    fn a_hidden_reminder_never_chimes_later() {
        let events = [live_event("a")];
        let mut toasts = MeetingToasts::default();
        tick(&mut toasts, &events, at(8, 56, 0), false);
        let update = tick_live(&mut toasts, &events, at(8, 58, 0));
        assert!(!update.chime);
        assert!(!tick(&mut toasts, &events, at(9, 0, 0), true).chime);
    }

    #[test]
    fn live_state_is_polled_every_thirty_seconds_inside_the_window() {
        let events = [live_event("a"), event("b")];
        let mut toasts = MeetingToasts::default();
        assert!(toasts.live_candidates(&events, at(8, 54, 0)).is_empty());
        assert_eq!(
            toasts.live_candidates(&events, at(8, 55, 0)),
            vec!["thread".to_owned()]
        );
        assert!(toasts.live_candidates(&events, at(8, 55, 29)).is_empty());
        assert_eq!(toasts.live_candidates(&events, at(8, 55, 30)).len(), 1);
        tick(&mut toasts, &events, at(8, 56, 0), true);
        let id = toasts.entries()[0].id;
        toasts.dismiss(id);
        assert!(toasts.live_candidates(&events, at(8, 57, 0)).is_empty());
    }

    #[test]
    fn a_hidden_reminder_is_spent_and_does_not_appear_later() {
        let events = [event("a")];
        let mut toasts = MeetingToasts::default();
        assert!(!tick(&mut toasts, &events, at(8, 56, 0), false).changed);
        assert!(!tick(&mut toasts, &events, at(8, 57, 0), true).changed);
        assert!(toasts.is_empty());
    }

    #[test]
    fn toasts_leave_when_the_window_closes_or_the_event_goes() {
        let events = [event("a")];
        let mut toasts = MeetingToasts::default();
        tick(&mut toasts, &events, at(8, 56, 0), true);
        assert!(tick(&mut toasts, &events, at(9, 5, 0), true).changed);
        assert!(toasts.is_empty());
        let mut removed = MeetingToasts::default();
        tick(&mut removed, &events, at(8, 56, 0), true);
        assert!(tick(&mut removed, &[], at(8, 57, 0), true).changed);
        assert!(removed.is_empty());
    }

    #[test]
    fn the_latest_toast_is_the_one_a_shortcut_joins() {
        let events = [event("a"), event("b")];
        let mut toasts = MeetingToasts::default();
        tick(&mut toasts, &events, at(8, 56, 0), true);
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
            event_id: "a".into(),
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
        assert_eq!(
            subtitle(&toast(ReminderPhase::Live), at(8, 57, 0), offset),
            "11:00 - live now"
        );
    }

    #[test]
    fn clearing_removes_every_open_toast() {
        let events = [event("a"), event("b")];
        let mut toasts = MeetingToasts::default();
        tick(&mut toasts, &events, at(8, 56, 0), true);
        assert!(toasts.clear());
        assert!(toasts.is_empty());
        assert!(!toasts.clear());
    }

    fn outlook_state(
        toasts: &mut MeetingToasts,
        now: DateTime<Utc>,
        fire_times: &[(&str, DateTime<Utc>)],
    ) {
        let outlook = toasts.outlook_mut();
        let (start, end) = outlook.poll_range(&[event("a")], now).unwrap();
        let reminders = fire_times
            .iter()
            .map(|(event_id, fire_time)| chatsvc::Reminder {
                event_id: (*event_id).into(),
                fire_time: *fire_time,
            })
            .collect();
        outlook.apply(start, end, Ok(reminders));
    }

    #[test]
    fn a_reminder_dismissed_in_outlook_is_not_shown_and_closes_an_open_toast() {
        let events = [event("a"), event("b")];
        let mut toasts = MeetingToasts::default();
        outlook_state(&mut toasts, at(8, 54, 0), &[("b", at(8, 45, 0))]);
        tick(&mut toasts, &events, at(8, 56, 0), true);
        assert_eq!(toasts.entries().len(), 1);
        assert_eq!(toasts.entries()[0].event_id, "b");
        outlook_state(&mut toasts, at(8, 57, 0), &[]);
        assert!(tick(&mut toasts, &events, at(8, 57, 1), true).changed);
        assert!(toasts.is_empty());
    }

    #[test]
    fn a_reminder_snoozed_in_outlook_comes_back_at_its_fire_time() {
        let events = [event("a")];
        let mut toasts = MeetingToasts::default();
        outlook_state(&mut toasts, at(8, 54, 0), &[("a", at(8, 45, 0))]);
        tick(&mut toasts, &events, at(8, 56, 0), true);
        assert_eq!(toasts.entries().len(), 1);
        outlook_state(&mut toasts, at(8, 57, 0), &[("a", at(8, 59, 0))]);
        tick(&mut toasts, &events, at(8, 57, 1), true);
        assert!(toasts.is_empty());
        tick(&mut toasts, &events, at(8, 58, 59), true);
        assert!(toasts.is_empty());
        tick(&mut toasts, &events, at(8, 59, 0), true);
        assert_eq!(toasts.entries().len(), 1);
    }

    #[test]
    fn snooze_and_dismiss_hand_back_the_event_for_the_outlook_write() {
        let events = [event("a"), event("b")];
        let mut toasts = MeetingToasts::default();
        tick(&mut toasts, &events, at(8, 56, 0), true);
        let snoozed = toasts.snooze(toasts.entries()[0].id).unwrap();
        assert_eq!(
            (snoozed.event_id.as_str(), snoozed.start),
            ("a", at(9, 0, 0))
        );
        let dismissed = toasts.dismiss(toasts.entries()[0].id).unwrap();
        assert_eq!(dismissed.event_id, "b");
    }

    #[test]
    fn checks_run_at_most_once_a_second() {
        let mut toasts = MeetingToasts::default();
        assert!(toasts.is_due_for_check(at(9, 0, 0)));
        tick(&mut toasts, &[], at(9, 0, 0), true);
        assert!(!toasts.is_due_for_check(at(9, 0, 0)));
        assert!(toasts.is_due_for_check(at(9, 0, 1)));
    }
}
