use std::collections::HashMap;

use chatsvc::{CalendarEvent, Reminder};
use chrono::{DateTime, Duration, Utc};
use gpui_kit::Context;

use super::reminder::{GRACE_MINUTES, LEAD_MINUTES, is_eligible};
use crate::app_state::AppState;
use crate::runtime;

const POLL_SECONDS: i64 = 60;
const WATCH_BEFORE_LEAD_MINUTES: i64 = 1;
const RANGE_HOURS: i64 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
struct KnownReminders {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    fire_times: HashMap<String, DateTime<Utc>>,
}

#[derive(Debug, Default)]
pub struct OutlookReminders {
    known: Option<KnownReminders>,
    last_poll: Option<DateTime<Utc>>,
    polling: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReminderWrite {
    Snooze(DateTime<Utc>),
    Dismiss,
}

fn watched(event: &CalendarEvent, now: DateTime<Utc>) -> bool {
    let watch_from = event.start - Duration::minutes(LEAD_MINUTES + WATCH_BEFORE_LEAD_MINUTES);
    let watch_until = event
        .end
        .min(event.start + Duration::minutes(GRACE_MINUTES));
    is_eligible(event) && now >= watch_from && now < watch_until
}

impl OutlookReminders {
    pub fn poll_range(
        &mut self,
        events: &[CalendarEvent],
        now: DateTime<Utc>,
    ) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
        let due = !self.polling
            && self
                .last_poll
                .is_none_or(|last| now - last >= Duration::seconds(POLL_SECONDS));
        if !due || !events.iter().any(|event| watched(event, now)) {
            return None;
        }
        self.polling = true;
        self.last_poll = Some(now);
        Some((
            now - Duration::hours(RANGE_HOURS),
            now + Duration::hours(RANGE_HOURS),
        ))
    }

    pub fn apply(
        &mut self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        fetched: Result<Vec<Reminder>, String>,
    ) {
        self.polling = false;
        self.known = fetched.ok().map(|reminders| KnownReminders {
            start,
            end,
            fire_times: reminders
                .into_iter()
                .map(|reminder| (reminder.event_id, reminder.fire_time))
                .collect(),
        });
    }

    pub fn allows(&self, event: &CalendarEvent, now: DateTime<Utc>) -> bool {
        let Some(known) = &self.known else {
            return true;
        };
        if event.start < known.start || event.start >= known.end {
            return true;
        }
        known
            .fire_times
            .get(&event.object_id)
            .is_some_and(|fire_time| now >= *fire_time)
    }

    pub fn note(&mut self, event_id: &str, write: ReminderWrite) {
        let Some(known) = self.known.as_mut() else {
            return;
        };
        match write {
            ReminderWrite::Snooze(until) => {
                known.fire_times.insert(event_id.to_owned(), until);
            }
            ReminderWrite::Dismiss => {
                known.fire_times.remove(event_id);
            }
        }
    }
}

impl AppState {
    pub(super) fn poll_outlook_reminders(&mut self, now: DateTime<Utc>, cx: &mut Context<Self>) {
        if self.mode.demo {
            return;
        }
        let Some(launcher) = self.call_launcher.clone() else {
            return;
        };
        let Some((start, end)) = self
            .meeting_toasts
            .outlook_mut()
            .poll_range(&self.calendar_events, now)
        else {
            return;
        };
        let fetched = runtime::spawn(async move { launcher.reminder_view(start, end).await });
        cx.spawn(async move |this, cx| {
            let fetched = fetched
                .await
                .unwrap_or_else(|_| Err("reminder fetch was dropped".to_owned()));
            if let Err(error) = &fetched {
                crate::diagnostics::record(format!(
                    "calendar: Outlook reminders not read: {error}"
                ));
            }
            this.update(cx, |state, _| {
                state
                    .meeting_toasts
                    .outlook_mut()
                    .apply(start, end, fetched);
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn write_outlook_reminder(&mut self, event_id: String, write: ReminderWrite) {
        if self.mode.demo || self.mode.read_only {
            return;
        }
        let Some(launcher) = self.call_launcher.clone() else {
            return;
        };
        self.meeting_toasts.outlook_mut().note(&event_id, write);
        runtime::handle().spawn(async move {
            let written = match write {
                ReminderWrite::Snooze(until) => launcher.snooze_reminder(&event_id, until).await,
                ReminderWrite::Dismiss => launcher.dismiss_reminder(&event_id).await,
            };
            if let Err(error) = written {
                crate::diagnostics::record(format!(
                    "calendar: Outlook reminder {write:?} not saved: {error}"
                ));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use chatsvc::ResponseType;
    use chrono::TimeZone;

    use super::*;

    fn at(hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 19, hour, minute, 0).unwrap()
    }

    fn event(id: &str) -> CalendarEvent {
        CalendarEvent {
            object_id: id.into(),
            subject: "Daily Standup".into(),
            start: at(9, 45),
            end: at(10, 0),
            all_day: false,
            cancelled: false,
            online: true,
            join_url: Some("https://teams.microsoft.com/l/meetup-join/x".into()),
            thread_id: None,
            organizer_name: None,
            organizer_address: None,
            ical_uid: None,
            response: ResponseType::Accepted,
        }
    }

    fn reminder(event_id: &str, fire_time: DateTime<Utc>) -> Reminder {
        Reminder {
            event_id: event_id.into(),
            fire_time,
        }
    }

    fn known(reminders: Vec<Reminder>, now: DateTime<Utc>) -> OutlookReminders {
        let mut outlook = OutlookReminders::default();
        let (start, end) = outlook.poll_range(&[event("a")], now).unwrap();
        outlook.apply(start, end, Ok(reminders));
        outlook
    }

    #[test]
    fn polls_only_near_a_meeting_and_once_a_minute() {
        let events = [event("a")];
        let mut outlook = OutlookReminders::default();
        assert_eq!(outlook.poll_range(&events, at(9, 38)), None);
        let range = outlook.poll_range(&events, at(9, 39)).unwrap();
        assert_eq!(range, (at(8, 39), at(10, 39)));
        assert_eq!(outlook.poll_range(&events, at(9, 41)), None);
        outlook.apply(range.0, range.1, Ok(Vec::new()));
        assert_eq!(
            outlook.poll_range(&events, at(9, 39) + Duration::seconds(59)),
            None
        );
        assert!(outlook.poll_range(&events, at(9, 40)).is_some());
        let mut later = OutlookReminders::default();
        assert_eq!(later.poll_range(&events, at(9, 50)), None);
    }

    #[test]
    fn unknown_state_keeps_the_local_behaviour() {
        let mut outlook = OutlookReminders::default();
        assert!(outlook.allows(&event("a"), at(9, 40)));
        let (start, end) = outlook.poll_range(&[event("a")], at(9, 39)).unwrap();
        outlook.apply(start, end, Err("offline".into()));
        assert!(outlook.allows(&event("a"), at(9, 40)));
    }

    #[test]
    fn a_reminder_missing_in_outlook_was_dismissed_there() {
        let outlook = known(vec![reminder("b", at(9, 30))], at(9, 39));
        assert!(!outlook.allows(&event("a"), at(9, 40)));
        assert!(outlook.allows(&event("b"), at(9, 40)));
    }

    #[test]
    fn a_reminder_snoozed_in_outlook_waits_for_its_fire_time() {
        let outlook = known(vec![reminder("a", at(9, 43))], at(9, 39));
        assert!(!outlook.allows(&event("a"), at(9, 42)));
        assert!(outlook.allows(&event("a"), at(9, 43)));
    }

    #[test]
    fn events_outside_the_read_range_are_not_judged() {
        let outlook = known(Vec::new(), at(9, 39));
        let tomorrow = CalendarEvent {
            start: at(9, 45) + Duration::days(1),
            ..event("a")
        };
        assert!(outlook.allows(&tomorrow, at(9, 40)));
    }

    #[test]
    fn own_writes_update_the_known_state_at_once() {
        let mut outlook = known(vec![reminder("a", at(9, 30))], at(9, 39));
        outlook.note("a", ReminderWrite::Snooze(at(9, 45)));
        assert!(!outlook.allows(&event("a"), at(9, 44)));
        assert!(outlook.allows(&event("a"), at(9, 45)));
        outlook.note("a", ReminderWrite::Dismiss);
        assert!(!outlook.allows(&event("a"), at(9, 46)));
    }
}
