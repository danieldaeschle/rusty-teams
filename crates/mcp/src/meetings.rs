use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use rmcp::schemars;

use crate::source::MeetingDraft;

pub const MEETINGS_DISABLED: &str =
    "organizing meetings is turned off (enable it in rusty-teams Settings > MCP)";
const MAX_AVAILABILITY_PEOPLE: usize = 20;
const MAX_AVAILABILITY_DAYS: i64 = 14;
const MAX_ATTENDEES: usize = 50;
const MAX_SUBJECT_CHARACTERS: usize = 255;
const MAX_DESCRIPTION_CHARACTERS: usize = 20_000;
const MAX_MEETING_HOURS: i64 = 24;
const PAST_TOLERANCE_MINUTES: i64 = 5;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FindAvailabilityParams {
    /// Email addresses of the people to check, up to 20. The user's own address works too.
    pub attendees: Vec<String>,
    /// Start of the window, ISO 8601 with offset.
    pub start: DateTime<Utc>,
    /// End of the window, ISO 8601 with offset, at most 14 days after start.
    pub end: DateTime<Utc>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateMeetingParams {
    /// Meeting title.
    pub subject: String,
    /// Start, ISO 8601 with offset.
    pub start: DateTime<Utc>,
    /// End, ISO 8601 with offset, at most 24 hours after start.
    pub end: DateTime<Utc>,
    /// Email addresses of the people to invite, up to 50.
    pub attendees: Vec<String>,
    /// Agenda or description, plain text, shown in the invitation.
    pub description: Option<String>,
    /// Your session id, so the user can tell meetings of different sessions apart.
    pub session_id: Option<String>,
    /// Your name, shown to the user next to the meeting.
    pub agent: Option<String>,
}

fn valid_email(address: &str) -> bool {
    let Some((local, domain)) = address.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !address.chars().any(|character| character.is_whitespace() || character == ',')
}

fn clean_emails(addresses: &[String], limit: usize) -> Result<Vec<String>, String> {
    let mut cleaned: Vec<String> = Vec::new();
    for address in addresses {
        let address = address.trim().to_lowercase();
        if !valid_email(&address) {
            return Err(format!("{address:?} is not an email address"));
        }
        if !cleaned.contains(&address) {
            cleaned.push(address);
        }
    }
    if cleaned.is_empty() {
        return Err("name at least one attendee".to_owned());
    }
    if cleaned.len() > limit {
        return Err(format!("at most {limit} attendees"));
    }
    Ok(cleaned)
}

pub fn availability_request(
    params: &FindAvailabilityParams,
) -> Result<(Vec<String>, DateTime<Utc>, DateTime<Utc>), String> {
    if params.end <= params.start {
        return Err("end must be after start".to_owned());
    }
    if params.end - params.start > Duration::days(MAX_AVAILABILITY_DAYS) {
        return Err(format!("the window may span at most {MAX_AVAILABILITY_DAYS} days"));
    }
    let emails = clean_emails(&params.attendees, MAX_AVAILABILITY_PEOPLE)?;
    Ok((emails, params.start, params.end))
}

pub fn meeting_draft(params: &CreateMeetingParams, now: DateTime<Utc>) -> Result<MeetingDraft, String> {
    let subject = params.subject.trim();
    if subject.is_empty() {
        return Err("subject is empty".to_owned());
    }
    if subject.chars().count() > MAX_SUBJECT_CHARACTERS {
        return Err(format!("subject is longer than {MAX_SUBJECT_CHARACTERS} characters"));
    }
    if params.end <= params.start {
        return Err("end must be after start".to_owned());
    }
    if params.end - params.start > Duration::hours(MAX_MEETING_HOURS) {
        return Err(format!("a meeting may last at most {MAX_MEETING_HOURS} hours"));
    }
    if params.start < now - Duration::minutes(PAST_TOLERANCE_MINUTES) {
        return Err("start lies in the past".to_owned());
    }
    let description = params.description.clone().unwrap_or_default();
    if description.chars().count() > MAX_DESCRIPTION_CHARACTERS {
        return Err(format!("description is longer than {MAX_DESCRIPTION_CHARACTERS} characters"));
    }
    Ok(MeetingDraft {
        subject: subject.to_owned(),
        start: params.start,
        end: params.end,
        attendees: clean_emails(&params.attendees, MAX_ATTENDEES)?,
        description,
    })
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};

    use super::{CreateMeetingParams, FindAvailabilityParams, availability_request, meeting_draft};

    fn params(start_hours: i64, length_hours: i64, attendees: &[&str]) -> CreateMeetingParams {
        let start = Utc.with_ymd_and_hms(2026, 11, 20, 9, 0, 0).unwrap() + Duration::hours(start_hours);
        CreateMeetingParams {
            subject: " Release review ".to_owned(),
            start,
            end: start + Duration::hours(length_hours),
            attendees: attendees.iter().map(|address| (*address).to_owned()).collect(),
            description: Some("Agenda".to_owned()),
            session_id: None,
            agent: None,
        }
    }

    #[test]
    fn a_meeting_draft_is_trimmed_and_deduplicated() {
        let now = Utc.with_ymd_and_hms(2026, 11, 12, 9, 0, 0).unwrap();
        let draft = meeting_draft(&params(0, 1, &["Mara@Example.com", "mara@example.com", "jonas@example.com"]), now).unwrap();
        assert_eq!(draft.subject, "Release review");
        assert_eq!(draft.attendees, ["mara@example.com", "jonas@example.com"]);
    }

    #[test]
    fn bad_meetings_are_refused() {
        let now = Utc.with_ymd_and_hms(2026, 11, 12, 9, 0, 0).unwrap();
        assert!(meeting_draft(&params(0, 0, &["a@example.com"]), now).is_err());
        assert!(meeting_draft(&params(0, 25, &["a@example.com"]), now).is_err());
        assert!(meeting_draft(&params(0, 1, &["not an address"]), now).is_err());
        assert!(meeting_draft(&params(0, 1, &[]), now).is_err());
        let late = Utc.with_ymd_and_hms(2026, 11, 21, 9, 0, 0).unwrap();
        assert!(meeting_draft(&params(0, 1, &["a@example.com"]), late).is_err());
    }

    #[test]
    fn an_availability_window_stays_short() {
        let start = Utc.with_ymd_and_hms(2026, 11, 20, 0, 0, 0).unwrap();
        let request = |days| FindAvailabilityParams {
            attendees: vec!["a@example.com".to_owned()],
            start,
            end: start + Duration::days(days),
        };
        assert!(availability_request(&request(14)).is_ok());
        assert!(availability_request(&request(15)).is_err());
    }
}
