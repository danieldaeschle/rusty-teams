use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use teams_core::{PersonProfile, WorkLocation, local_time};

pub const PROFILE_MAX_AGE: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContactKind {
    Email,
    WorkPhone,
    MobilePhone,
    Office,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactRow {
    pub kind: ContactKind,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusBox {
    pub text: String,
    pub out_of_office: bool,
}

pub fn status_line(availability: &str, local_time: Option<&str>) -> String {
    match (availability.is_empty(), local_time) {
        (false, Some(time)) => format!("{availability} - {time} local time"),
        (false, None) => availability.to_owned(),
        (true, Some(time)) => format!("{time} local time"),
        (true, None) => String::new(),
    }
}

pub fn local_time_label(profile: &PersonProfile, now: DateTime<Utc>) -> Option<String> {
    let zone = profile.time_zone.as_deref()?;
    Some(local_time(zone, now)?.format("%H:%M").to_string())
}

pub fn work_location_label(location: WorkLocation) -> &'static str {
    crate::own_status::work_location_label(Some(location))
}

pub fn status_box(profile: &PersonProfile) -> Option<StatusBox> {
    if let Some(text) = &profile.out_of_office {
        let text = match text.trim() {
            "" => "Out of office".to_owned(),
            text => text.to_owned(),
        };
        return Some(StatusBox {
            text,
            out_of_office: true,
        });
    }
    profile.status_message.clone().map(|text| StatusBox {
        text,
        out_of_office: false,
    })
}

pub fn contact_rows(profile: &PersonProfile) -> Vec<ContactRow> {
    let present = |value: &Option<String>| value.clone().filter(|value| !value.trim().is_empty());
    let mut rows = Vec::new();
    let mut push = |kind: ContactKind, value: Option<String>| {
        if let Some(value) = value {
            rows.push(ContactRow { kind, value });
        }
    };
    push(ContactKind::Email, present(&profile.email));
    for phone in &profile.work_phones {
        push(ContactKind::WorkPhone, present(&Some(phone.clone())));
    }
    push(ContactKind::MobilePhone, present(&profile.mobile_phone));
    push(ContactKind::Office, present(&profile.office_location));
    rows
}

pub fn has_organization(profile: &PersonProfile) -> bool {
    profile.manager.is_some() || !profile.direct_reports.is_empty()
}

pub fn mailto_url(email: &str) -> String {
    format!("mailto:{email}")
}

pub fn is_stale(fetched_at: Instant, now: Instant) -> bool {
    now.duration_since(fetched_at) >= PROFILE_MAX_AGE
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use teams_core::{OrgPerson, WorkLocationKind, WorkLocationSource};

    use super::*;

    fn profile() -> PersonProfile {
        PersonProfile {
            user_id: "u".into(),
            display_name: Some("Ada".into()),
            job_title: None,
            department: None,
            company: None,
            office_location: None,
            email: None,
            work_phones: Vec::new(),
            mobile_phone: None,
            manager: None,
            direct_reports: Vec::new(),
            time_zone: None,
            availability: None,
            status_message: None,
            out_of_office: None,
            work_location: None,
        }
    }

    #[test]
    fn status_line_joins_availability_and_local_time() {
        assert_eq!(
            status_line("Available", Some("14:35")),
            "Available - 14:35 local time"
        );
        assert_eq!(status_line("Busy", None), "Busy");
        assert_eq!(status_line("", Some("09:05")), "09:05 local time");
        assert_eq!(status_line("", None), "");
    }

    #[test]
    fn local_time_needs_a_known_zone() {
        let now = Utc.with_ymd_and_hms(2026, 10, 9, 12, 0, 0).unwrap();
        let mut person = profile();
        assert_eq!(local_time_label(&person, now), None);
        person.time_zone = Some("W. Europe Standard Time".into());
        assert_eq!(local_time_label(&person, now).as_deref(), Some("14:00"));
        person.time_zone = Some("Unknown Zone".into());
        assert_eq!(local_time_label(&person, now), None);
    }

    #[test]
    fn work_location_labels_follow_the_source() {
        let at = |kind, source| work_location_label(WorkLocation { kind, source });
        assert_eq!(
            at(WorkLocationKind::Office, WorkLocationSource::Set),
            "In the office"
        );
        assert_eq!(
            at(WorkLocationKind::Office, WorkLocationSource::Scheduled),
            "Planned in the office"
        );
        assert_eq!(
            at(WorkLocationKind::Office, WorkLocationSource::Verified),
            "Checked in to the office"
        );
        assert_eq!(
            at(WorkLocationKind::Remote, WorkLocationSource::Set),
            "Working remotely"
        );
        assert_eq!(
            at(WorkLocationKind::Remote, WorkLocationSource::Scheduled),
            "Planned to work remotely"
        );
    }

    #[test]
    fn out_of_office_replaces_the_status_message() {
        let mut person = profile();
        person.status_message = Some("Focus".into());
        assert_eq!(
            status_box(&person),
            Some(StatusBox {
                text: "Focus".into(),
                out_of_office: false
            })
        );
        person.out_of_office = Some("Back Monday".into());
        assert_eq!(
            status_box(&person),
            Some(StatusBox {
                text: "Back Monday".into(),
                out_of_office: true
            })
        );
        person.out_of_office = Some(" ".into());
        assert_eq!(status_box(&person).unwrap().text, "Out of office");
    }

    #[test]
    fn contact_rows_hide_empty_fields() {
        let mut person = profile();
        person.email = Some("ada@example.com".into());
        person.work_phones = vec!["+49 1".into(), " ".into()];
        person.mobile_phone = Some(String::new());
        person.office_location = Some("Berlin".into());
        let kinds: Vec<ContactKind> = contact_rows(&person)
            .into_iter()
            .map(|row| row.kind)
            .collect();
        assert_eq!(
            kinds,
            vec![
                ContactKind::Email,
                ContactKind::WorkPhone,
                ContactKind::Office
            ]
        );
        assert!(contact_rows(&profile()).is_empty());
    }

    #[test]
    fn organization_needs_a_manager_or_reports() {
        let mut person = profile();
        assert!(!has_organization(&person));
        person.direct_reports.push(OrgPerson::default());
        assert!(has_organization(&person));
    }

    #[test]
    fn profiles_go_stale_after_ten_minutes() {
        let start = Instant::now();
        assert!(!is_stale(start, start + Duration::from_secs(599)));
        assert!(is_stale(start, start + Duration::from_secs(600)));
    }
}
