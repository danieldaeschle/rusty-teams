use chatsvc::{Attendee, CalendarEvent, Repeat, Reply, ResponseType};
use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike, Utc};
use gpui_kit::Context;
use tokio::sync::oneshot;

use super::actions::NOT_CONNECTED_NOTICE;
use super::agenda::demo_join_url;
use crate::app_state::AppState;
use crate::runtime;

const HALF_HOUR_MINUTES: i64 = 30;
const DEFAULT_LENGTH_MINUTES: i64 = 30;
const READ_ONLY_MESSAGE: &str = "Meetings cannot be scheduled in read-only mode.";
const RESPONSE_FAILED_NOTICE: &str = "Could not send your response";
const DELETE_FAILED_NOTICE: &str = "Could not delete the meeting";
const DELETE_READ_ONLY_NOTICE: &str = "Meetings cannot be deleted in read-only mode.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invitee {
    pub user_id: String,
    pub name: String,
    pub mail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingDraft {
    pub subject: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub local_start: NaiveDateTime,
    pub local_end: NaiveDateTime,
    pub time_zone_name: Option<String>,
    pub repeat: Repeat,
    pub details: String,
    pub people: Vec<Invitee>,
}

pub fn default_window(now: DateTime<Utc>, zone: &impl TimeZone) -> (NaiveDateTime, NaiveDateTime) {
    let local = now.with_timezone(zone).naive_local();
    let past_half_hour = i64::from(local.minute()) % HALF_HOUR_MINUTES;
    let start = local
        .with_second(0)
        .and_then(|time| time.with_nanosecond(0))
        .unwrap_or(local)
        - Duration::minutes(past_half_hour)
        + Duration::minutes(HALF_HOUR_MINUTES);
    (start, start + Duration::minutes(DEFAULT_LENGTH_MINUTES))
}

fn utc_of(zone: &impl TimeZone, date: NaiveDate, time: NaiveTime) -> Option<DateTime<Utc>> {
    zone.from_local_datetime(&date.and_time(time))
        .earliest()
        .map(|moment| moment.with_timezone(&Utc))
}

#[derive(Debug, Clone)]
pub struct FormValues {
    pub subject: String,
    pub date: Option<NaiveDate>,
    pub start: NaiveTime,
    pub end: NaiveTime,
    pub repeat: Repeat,
    pub details: String,
    pub people: Vec<Invitee>,
}

pub fn draft_from_form(
    values: FormValues,
    zone: &impl TimeZone,
    time_zone_name: Option<String>,
) -> Result<MeetingDraft, String> {
    let subject = values.subject.trim();
    if subject.is_empty() {
        return Err("Add a title.".to_owned());
    }
    let date = values.date.ok_or_else(|| "Pick a date.".to_owned())?;
    let start = utc_of(zone, date, values.start)
        .ok_or_else(|| "That start time does not exist.".to_owned())?;
    let end =
        utc_of(zone, date, values.end).ok_or_else(|| "That end time does not exist.".to_owned())?;
    if end <= start {
        return Err("The end must be after the start.".to_owned());
    }
    Ok(MeetingDraft {
        subject: subject.to_owned(),
        start,
        end,
        local_start: date.and_time(values.start),
        local_end: date.and_time(values.end),
        time_zone_name,
        repeat: values.repeat,
        details: values.details.trim().to_owned(),
        people: values.people,
    })
}

pub fn attendee_for(invitee: &Invitee, looked_up: Option<String>) -> Result<Attendee, String> {
    let address = invitee
        .mail
        .clone()
        .or(looked_up)
        .ok_or_else(|| format!("No email address found for {}.", invitee.name))?;
    Ok(Attendee {
        name: invitee.name.clone(),
        address,
    })
}

pub fn response_for(reply: Reply) -> ResponseType {
    match reply {
        Reply::Accept => ResponseType::Accepted,
        Reply::Tentative => ResponseType::Tentative,
        Reply::Decline => ResponseType::Declined,
    }
}

fn demo_event(draft: &MeetingDraft, serial: u64) -> CalendarEvent {
    CalendarEvent {
        object_id: format!("demo-new-{serial}"),
        subject: draft.subject.clone(),
        start: draft.start,
        end: draft.end,
        all_day: false,
        cancelled: false,
        online: true,
        join_url: Some(demo_join_url("demo-new")),
        thread_id: None,
        organizer_name: Some("You".to_owned()),
        organizer_address: None,
        ical_uid: None,
        show_as: Default::default(),
        dial_in: None,
        response: ResponseType::Organizer,
        ..Default::default()
    }
}

impl AppState {
    pub fn load_own_time_zone(&mut self, cx: &mut Context<Self>) {
        if self.own_time_zone.is_some() || self.own_time_zone_requested || self.own_email.is_empty()
        {
            return;
        }
        let Some(launcher) = self.call_launcher.clone() else {
            return;
        };
        self.own_time_zone_requested = true;
        let mail = self.own_email.clone();
        let looked_up = runtime::spawn(async move { launcher.time_zone_name(&mail).await });
        cx.spawn(async move |this, cx| {
            let time_zone_name = looked_up.await.ok().flatten();
            this.update(cx, |state, _| {
                state.own_time_zone_requested = time_zone_name.is_some();
                state.own_time_zone = time_zone_name;
            })
            .ok();
        })
        .detach();
    }

    pub fn create_meeting(
        &mut self,
        draft: MeetingDraft,
        cx: &mut Context<Self>,
    ) -> oneshot::Receiver<Result<(), String>> {
        let (sender, receiver) = oneshot::channel();
        if self.mode.demo {
            let event = demo_event(&draft, self.demo_meetings.len() as u64 + 1);
            self.demo_meetings.push(event);
            self.refresh_calendar(cx);
            let _ = sender.send(Ok(()));
            return receiver;
        }
        if self.mode.read_only {
            let _ = sender.send(Err(READ_ONLY_MESSAGE.to_owned()));
            return receiver;
        }
        let Some(launcher) = self.call_launcher.clone() else {
            let _ = sender.send(Err(NOT_CONNECTED_NOTICE.to_owned()));
            return receiver;
        };
        let accounts = store::load_calendar_accounts(&self.store);
        let store = self.store.clone();
        let created = runtime::spawn(async move {
            super::meeting_create::create_across_targets(launcher, draft, accounts, store).await
        });
        cx.spawn(async move |this, cx| {
            let outcome = created
                .await
                .unwrap_or_else(|_| Err("The request was cancelled.".to_owned()))
                .map(|_| ());
            if outcome.is_ok() {
                this.update(cx, |state, cx| state.refresh_calendar(cx)).ok();
            }
            let _ = sender.send(outcome);
        })
        .detach();
        receiver
    }

    pub fn toggle_rsvp_send_response(&mut self, cx: &mut Context<Self>) {
        self.rsvp_send_response = !self.rsvp_send_response;
        cx.notify();
    }

    fn set_event_response(&mut self, event_id: &str, response: ResponseType) {
        for event in self
            .calendar_events
            .iter_mut()
            .chain(self.calendar_week.events.iter_mut())
            .filter(|event| event.object_id == event_id)
        {
            event.response = response;
        }
        self.calls_list.invalidate();
    }

    fn remove_calendar_event(&mut self, event_id: &str) -> Option<CalendarEvent> {
        let mut removed = None;
        if let Some(index) = self
            .calendar_week
            .events
            .iter()
            .position(|event| event.object_id == event_id)
        {
            removed = Some(self.calendar_week.events.remove(index));
        }
        if let Some(index) = self
            .calendar_events
            .iter()
            .position(|event| event.object_id == event_id)
        {
            let event = self.calendar_events.remove(index);
            if removed.is_none() {
                removed = Some(event);
            }
        }
        self.calls_list.invalidate();
        removed
    }

    fn restore_calendar_event(&mut self, event: CalendarEvent) {
        if !self
            .calendar_week
            .events
            .iter()
            .any(|existing| existing.object_id == event.object_id)
        {
            self.calendar_week.events.push(event.clone());
            self.calendar_week
                .events
                .sort_by_key(|item| (item.start, item.end));
        }
        if !self
            .calendar_events
            .iter()
            .any(|existing| existing.object_id == event.object_id)
        {
            self.calendar_events.push(event);
            self.calendar_events
                .sort_by_key(|item| (item.start, item.end));
        }
        self.calls_list.invalidate();
    }

    pub fn delete_calendar_event(&mut self, event_key: &str, cx: &mut Context<Self>) {
        if self.mode.read_only {
            self.raise_notice(DELETE_READ_ONLY_NOTICE.to_owned(), None, cx);
            return;
        }
        let Some(event) = self
            .calendar_events
            .iter()
            .chain(&self.calendar_week.events)
            .find(|event| event.object_id == event_key)
            .cloned()
        else {
            return;
        };
        let snapshot = event.clone();
        let (event_id, ical_uid) = (event.object_id.clone(), event.ical_uid.clone());
        let source = event.source;
        let account_id = event.account_id.clone();
        let calendar_id = event.calendar_id.clone();
        self.remove_calendar_event(&event_id);
        cx.notify();
        let launcher = self.call_launcher.clone().filter(|_| !self.mode.demo);
        let Some(launcher) = launcher else {
            return;
        };
        let send_updates = self.rsvp_send_response;
        let store = self.store.clone();
        let deleted = runtime::spawn({
            let event_id = event_id.clone();
            async move {
                match source {
                    chatsvc::CalendarSource::Outlook => {
                        launcher
                            .delete_calendar_event(&event_id, ical_uid.as_deref())
                            .await
                    }
                    chatsvc::CalendarSource::Google => {
                        let mut accounts = store::load_calendar_accounts(&store);
                        let account =
                            account_id.ok_or_else(|| "Google account missing".to_owned())?;
                        let calendar = calendar_id.unwrap_or_else(|| "primary".to_owned());
                        let result = crate::google_calendar_sync::delete_google_event(
                            &mut accounts,
                            &account,
                            &calendar,
                            &event_id,
                            send_updates,
                        )
                        .await;
                        let _ = store::save_calendar_accounts(&store, &accounts);
                        result
                    }
                }
            }
        });
        cx.spawn(async move |this, cx| {
            let ok = deleted.await.is_ok_and(|result| result.is_ok());
            this.update(cx, |state, cx| {
                if ok {
                    // refresh_calendar nudges agenda poll + reloads the week grid.
                    state.refresh_calendar(cx);
                    return;
                }
                state.restore_calendar_event(snapshot);
                state.raise_notice(DELETE_FAILED_NOTICE.to_owned(), None, cx);
            })
            .ok();
        })
        .detach();
    }

    pub fn respond_to_event(&mut self, event_key: &str, reply: Reply, cx: &mut Context<Self>) {
        if self.mode.read_only {
            self.raise_notice(RESPONSE_FAILED_NOTICE.to_owned(), None, cx);
            return;
        }
        let Some(event) = self
            .calendar_events
            .iter()
            .chain(&self.calendar_week.events)
            .find(|event| event.object_id == event_key)
        else {
            return;
        };
        let previous = event.response;
        let (event_id, ical_uid) = (event.object_id.clone(), event.ical_uid.clone());
        let source = event.source;
        let account_id = event.account_id.clone();
        let calendar_id = event.calendar_id.clone();
        self.set_event_response(&event_id, response_for(reply));
        cx.notify();
        let launcher = self.call_launcher.clone().filter(|_| !self.mode.demo);
        let Some(launcher) = launcher else {
            return;
        };
        let send_response = self.rsvp_send_response;
        let store = self.store.clone();
        let answered = runtime::spawn({
            let event_id = event_id.clone();
            async move {
                match source {
                    chatsvc::CalendarSource::Outlook => {
                        launcher
                            .respond_to_event(&event_id, ical_uid.as_deref(), reply, send_response)
                            .await
                    }
                    chatsvc::CalendarSource::Google => {
                        let mut accounts = store::load_calendar_accounts(&store);
                        let account =
                            account_id.ok_or_else(|| "Google account missing".to_owned())?;
                        let calendar = calendar_id.unwrap_or_else(|| "primary".to_owned());
                        let result = crate::google_calendar_sync::respond_google_event(
                            &mut accounts,
                            &account,
                            &calendar,
                            &event_id,
                            reply,
                            send_response,
                        )
                        .await;
                        let _ = store::save_calendar_accounts(&store, &accounts);
                        result
                    }
                }
            }
        });
        cx.spawn(async move |this, cx| {
            let sent = answered.await.is_ok_and(|result| result.is_ok());
            this.update(cx, |state, cx| {
                if sent {
                    state.refresh_calendar(cx);
                    return;
                }
                state.set_event_response(&event_id, previous);
                state.raise_notice(RESPONSE_FAILED_NOTICE.to_owned(), None, cx);
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use chrono::FixedOffset;

    use super::*;

    fn utc(hour: u32, minute: u32, second: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 13, hour, minute, second)
            .unwrap()
    }

    fn plus_two() -> FixedOffset {
        FixedOffset::east_opt(2 * 3600).unwrap()
    }

    fn time(hour: u32, minute: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(hour, minute, 0).unwrap()
    }

    fn day() -> Option<NaiveDate> {
        NaiveDate::from_ymd_opt(2026, 10, 14)
    }

    fn invitee(name: &str, mail: Option<&str>) -> Invitee {
        Invitee {
            user_id: format!("id-{name}"),
            name: name.to_owned(),
            mail: mail.map(str::to_owned),
        }
    }

    #[test]
    fn the_default_window_starts_at_the_next_half_hour_and_lasts_thirty_minutes() {
        let (start, end) = default_window(utc(8, 12, 40), &plus_two());
        assert_eq!(start.to_string(), "2026-10-13 10:30:00");
        assert_eq!(end.to_string(), "2026-10-13 11:00:00");
        let (start, _) = default_window(utc(8, 40, 0), &plus_two());
        assert_eq!(start.to_string(), "2026-10-13 11:00:00");
    }

    #[test]
    fn a_time_on_the_half_hour_moves_to_the_following_one() {
        let (start, _) = default_window(utc(8, 30, 0), &plus_two());
        assert_eq!(start.to_string(), "2026-10-13 11:00:00");
        let (start, _) = default_window(utc(8, 0, 0), &plus_two());
        assert_eq!(start.to_string(), "2026-10-13 10:30:00");
    }

    #[test]
    fn the_late_evening_default_rolls_into_the_next_day() {
        let (start, end) = default_window(utc(21, 45, 0), &plus_two());
        assert_eq!(start.to_string(), "2026-10-14 00:00:00");
        assert_eq!(end.to_string(), "2026-10-14 00:30:00");
    }

    #[test]
    fn the_form_turns_local_times_into_utc() {
        let draft = draft_from_form(
            FormValues {
                subject: " Planning ".into(),
                date: day(),
                start: time(11, 0),
                end: time(11, 30),
                repeat: Repeat::Weekly,
                details: "  Agenda  ".into(),
                people: Vec::new(),
            },
            &plus_two(),
            Some("W. Europe Standard Time".into()),
        )
        .unwrap();
        assert_eq!(draft.subject, "Planning");
        assert_eq!(draft.details, "Agenda");
        assert_eq!(
            draft.start,
            Utc.with_ymd_and_hms(2026, 10, 14, 9, 0, 0).unwrap()
        );
        assert_eq!(
            draft.end,
            Utc.with_ymd_and_hms(2026, 10, 14, 9, 30, 0).unwrap()
        );
        assert_eq!(draft.repeat, Repeat::Weekly);
        assert_eq!(draft.local_start.to_string(), "2026-10-14 11:00:00");
        assert_eq!(draft.local_end.to_string(), "2026-10-14 11:30:00");
        assert_eq!(
            draft.time_zone_name.as_deref(),
            Some("W. Europe Standard Time")
        );
    }

    #[test]
    fn the_form_rejects_a_missing_title_date_or_backwards_times() {
        let build = |subject: &str, date, start, end| {
            let values = FormValues {
                subject: subject.into(),
                date,
                start,
                end,
                repeat: Repeat::Never,
                details: String::new(),
                people: Vec::new(),
            };
            draft_from_form(values, &plus_two(), None)
        };
        assert_eq!(
            build(" ", day(), time(9, 0), time(10, 0)).unwrap_err(),
            "Add a title."
        );
        assert_eq!(
            build("x", None, time(9, 0), time(10, 0)).unwrap_err(),
            "Pick a date."
        );
        assert_eq!(
            build("x", day(), time(10, 0), time(10, 0)).unwrap_err(),
            "The end must be after the start."
        );
        assert!(build("x", day(), time(11, 0), time(10, 0)).is_err());
    }

    #[test]
    fn an_invitee_uses_the_known_address_before_a_looked_up_one() {
        let known = invitee("Mara", Some("mara@example.invalid"));
        let attendee = attendee_for(&known, Some("other@example.invalid".into())).unwrap();
        assert_eq!(attendee.address, "mara@example.invalid");
        let unknown = invitee("Jonas", None);
        let attendee = attendee_for(&unknown, Some("jonas@example.invalid".into())).unwrap();
        assert_eq!(attendee.address, "jonas@example.invalid");
        assert_eq!(attendee.name, "Jonas");
    }

    #[test]
    fn an_invitee_without_any_address_is_an_error_naming_the_person() {
        let error = attendee_for(&invitee("Jonas", None), None).unwrap_err();
        assert_eq!(error, "No email address found for Jonas.");
    }

    #[test]
    fn replies_map_to_the_shown_response() {
        assert_eq!(response_for(Reply::Accept), ResponseType::Accepted);
        assert_eq!(response_for(Reply::Tentative), ResponseType::Tentative);
        assert_eq!(response_for(Reply::Decline), ResponseType::Declined);
    }
}
