//! Read-only calendars of people in the organization, from their free/busy schedule.

use app_base::ui_locale::tr;
use chrono::{DateTime, Utc};
use store::{CalendarAccounts, PersonCalendar};

use chatsvc::{Calendar, CalendarEvent, CalendarSource, PersonSchedule, ShowAs};

/// Graph's getSchedule limit per request.
const SCHEDULES_PER_REQUEST: usize = 20;

/// Events plus per-person failure messages.
pub async fn fetch_people_events(
    calendar: &Calendar,
    accounts: &CalendarAccounts,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<(Vec<CalendarEvent>, Vec<String>), String> {
    let people: Vec<&PersonCalendar> = accounts
        .people_calendars
        .iter()
        .filter(|person| person.selected)
        .collect();
    let mut events = Vec::new();
    let mut failures = Vec::new();
    for chunk in people.chunks(SCHEDULES_PER_REQUEST) {
        let mails: Vec<String> = chunk.iter().map(|person| person.mail.clone()).collect();
        let schedules = calendar
            .schedules(&mails, start, end)
            .await
            .map_err(|error| error.to_string())?;
        for person in chunk {
            let Some(schedule) = schedules
                .iter()
                .find(|schedule| schedule.email.eq_ignore_ascii_case(&person.mail))
            else {
                continue;
            };
            match &schedule.error {
                Some(error) => failures.push(format!("{}: {error}", person.name)),
                None => events.extend(person_events(person, schedule)),
            }
        }
    }
    Ok((events, failures))
}

pub fn person_events(person: &PersonCalendar, schedule: &PersonSchedule) -> Vec<CalendarEvent> {
    schedule
        .busy
        .iter()
        .enumerate()
        .map(|(index, slot)| CalendarEvent {
            object_id: format!("person:{}:{}:{index}", person.mail, slot.start.timestamp()),
            subject: slot
                .subject
                .clone()
                .unwrap_or_else(|| status_text(slot.show_as())),
            start: slot.start,
            end: slot.end,
            show_as: slot.show_as(),
            location: slot.location.clone(),
            source: CalendarSource::Person,
            sources: vec![CalendarSource::Person],
            account_id: Some(person.mail.clone()),
            calendar_name: Some(person.name.clone()),
            ..Default::default()
        })
        .collect()
}

fn status_text(show_as: ShowAs) -> String {
    tr(match show_as {
        ShowAs::Tentative => "calendar.view.person_tentative",
        ShowAs::OutOfOffice => "calendar.view.person_away",
        ShowAs::WorkingElsewhere => "calendar.view.person_elsewhere",
        _ => "calendar.view.person_busy",
    })
}

#[cfg(test)]
mod tests {
    use chatsvc::BusyTime;
    use chrono::TimeZone;

    use super::*;

    fn slot(subject: Option<&str>, status: &str) -> BusyTime {
        let start = Utc.with_ymd_and_hms(2026, 11, 24, 9, 0, 0).unwrap();
        BusyTime {
            start,
            end: start + chrono::Duration::hours(1),
            status: status.to_owned(),
            subject: subject.map(str::to_owned),
            location: None,
        }
    }

    #[test]
    fn slots_become_read_only_person_events() {
        let person = PersonCalendar {
            mail: "mara@example.com".into(),
            name: "Mara".into(),
            selected: true,
            color: None,
        };
        let schedule = PersonSchedule {
            email: "mara@example.com".into(),
            busy: vec![slot(Some("Planning"), "busy"), slot(None, "oof")],
            error: None,
        };
        let events = person_events(&person, &schedule);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].subject, "Planning");
        assert_eq!(events[0].source, CalendarSource::Person);
        assert_eq!(events[0].calendar_name.as_deref(), Some("Mara"));
        assert_eq!(events[0].account_id.as_deref(), Some("mara@example.com"));
        assert!(events[0].join_url.is_none());
        assert_eq!(events[1].show_as, ShowAs::OutOfOffice);
        assert!(!events[1].subject.is_empty());
        assert_ne!(events[0].object_id, events[1].object_id);
    }
}
