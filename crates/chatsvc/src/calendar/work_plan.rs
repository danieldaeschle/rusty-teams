use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use serde_json::{Value, json};
use session::{GRAPH, Method, Request, Scope};

use super::Calendar;
use super::schedule::CALENDARS_SCOPE;
use crate::cards::CardTransport;
use crate::error::{Error, Result};

const READ_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.f";

/// Where the work plan in Outlook puts the user on a day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkPlanLocation {
    Office,
    Remote,
    TimeOff,
    Unspecified,
}

impl WorkPlanLocation {
    fn from_graph(value: &str) -> Self {
        match value {
            "office" => WorkPlanLocation::Office,
            "remote" => WorkPlanLocation::Remote,
            "timeOff" => WorkPlanLocation::TimeOff,
            _ => WorkPlanLocation::Unspecified,
        }
    }

    pub fn as_graph(self) -> &'static str {
        match self {
            WorkPlanLocation::Office => "office",
            WorkPlanLocation::Remote => "remote",
            WorkPlanLocation::TimeOff => "timeOff",
            WorkPlanLocation::Unspecified => "unspecified",
        }
    }
}

/// One day of the work plan, with its working hours in the mailbox's time zone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkPlanDay {
    pub id: String,
    pub date: NaiveDate,
    pub location: WorkPlanLocation,
    pub start: NaiveTime,
    pub end: NaiveTime,
    /// The occurrence as Graph sent it; a change is written back as the whole object.
    pub occurrence: Value,
}

fn settings_url() -> String {
    format!("{GRAPH}/beta/me/settings/workHoursAndLocations")
}

pub fn work_plan_url(from: NaiveDate, to: NaiveDate) -> String {
    format!(
        "{}/occurrencesView(startDateTime='{from}T00:00:00Z',endDateTime='{to}T00:00:00Z')",
        settings_url()
    )
}

fn occurrence_url(id: &str) -> String {
    format!(
        "{}/occurrences/{}",
        settings_url(),
        crate::messages::encode(id)
    )
}

/// Graph updates an occurrence with PUT only, and rejects one without its recurrenceId.
pub fn occurrence_body(day: &WorkPlanDay, location: WorkPlanLocation) -> Value {
    let mut body = json!({
        "recurrenceId": day.occurrence["recurrenceId"],
        "start": day.occurrence["start"],
        "end": day.occurrence["end"],
        "workLocationType": location.as_graph(),
    });
    if location == WorkPlanLocation::Office && !day.occurrence["placeId"].is_null() {
        body["placeId"] = day.occurrence["placeId"].clone();
    }
    body
}

fn local(value: &Value) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(value["dateTime"].as_str()?, READ_FORMAT).ok()
}

pub fn parse_work_plan(body: &Value) -> Vec<WorkPlanDay> {
    body["value"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let (start, end) = (local(&item["start"])?, local(&item["end"])?);
            Some(WorkPlanDay {
                id: item["id"].as_str()?.to_owned(),
                date: start.date(),
                location: WorkPlanLocation::from_graph(
                    item["workLocationType"].as_str().unwrap_or_default(),
                ),
                start: start.time(),
                end: end.time(),
                occurrence: item.clone(),
            })
        })
        .collect()
}

impl<T: CardTransport> Calendar<T> {
    /// The user's planned work locations for the days from `from` up to `to`.
    pub async fn work_plan(&self, from: NaiveDate, to: NaiveDate) -> Result<Vec<WorkPlanDay>> {
        let url = work_plan_url(from, to);
        let answer = self
            .transport
            .send(Request::get(&url), Scope::graph(CALENDARS_SCOPE))
            .await?;
        if !answer.is_success() {
            return Err(Error::Session(session::Error::api(
                answer.status,
                &url,
                answer.body,
            )));
        }
        Ok(parse_work_plan(&answer.body))
    }

    /// Changes where the user works on one day of the plan.
    pub async fn set_work_plan_location(
        &self,
        day: &WorkPlanDay,
        location: WorkPlanLocation,
    ) -> Result<()> {
        let url = occurrence_url(&day.id);
        let request = Request::with_body(Method::Put, &url, occurrence_body(day, location));
        let answer = self
            .transport
            .send(request, Scope::graph(CALENDARS_SCOPE))
            .await?;
        if !answer.is_success() {
            return Err(Error::Session(session::Error::api(
                answer.status,
                &url,
                answer.body,
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, NaiveTime};
    use serde_json::json;

    use super::{WorkPlanLocation, occurrence_body, parse_work_plan, work_plan_url};

    #[test]
    fn work_plan_days_keep_their_location_and_hours() {
        let body = json!({"value": [
            {"id": "a", "workLocationType": "remote",
             "start": {"dateTime": "2026-11-23T08:00:00.0000000", "timeZone": "W. Europe Standard Time"},
             "end": {"dateTime": "2026-11-23T17:00:00.0000000", "timeZone": "W. Europe Standard Time"}},
            {"id": "b", "workLocationType": "office",
             "start": {"dateTime": "2026-11-24T07:00:00.0000000"},
             "end": {"dateTime": "2026-11-24T16:30:00.0000000"}},
            {"id": "c", "workLocationType": "somethingNew",
             "start": {"dateTime": "2026-11-25T07:00:00.0000000"},
             "end": {"dateTime": "2026-11-25T16:30:00.0000000"}},
            {"id": "broken", "workLocationType": "office"}
        ]});
        let days = parse_work_plan(&body);
        assert_eq!(days.len(), 3);
        assert_eq!(days[0].date, NaiveDate::from_ymd_opt(2026, 11, 23).unwrap());
        assert_eq!(days[0].location, WorkPlanLocation::Remote);
        assert_eq!(days[1].location, WorkPlanLocation::Office);
        assert_eq!(days[1].start, NaiveTime::from_hms_opt(7, 0, 0).unwrap());
        assert_eq!(days[1].end, NaiveTime::from_hms_opt(16, 30, 0).unwrap());
        assert_eq!(days[2].location, WorkPlanLocation::Unspecified);
    }

    #[test]
    fn a_change_is_the_whole_occurrence_with_its_recurrence() {
        let day = &parse_work_plan(&json!({"value": [
            {"id": "a", "recurrenceId": "r", "workLocationType": "office", "placeId": "desk",
             "start": {"dateTime": "2026-11-30T08:00:00.0000000", "timeZone": "W. Europe Standard Time"},
             "end": {"dateTime": "2026-11-30T17:00:00.0000000", "timeZone": "W. Europe Standard Time"}}
        ]}))[0];
        assert_eq!(
            occurrence_body(day, WorkPlanLocation::Remote),
            json!({
                "recurrenceId": "r",
                "start": {"dateTime": "2026-11-30T08:00:00.0000000", "timeZone": "W. Europe Standard Time"},
                "end": {"dateTime": "2026-11-30T17:00:00.0000000", "timeZone": "W. Europe Standard Time"},
                "workLocationType": "remote"
            })
        );
        assert_eq!(
            occurrence_body(day, WorkPlanLocation::Office)["placeId"],
            "desk"
        );
    }

    #[test]
    fn the_view_covers_whole_days() {
        let url = work_plan_url(
            NaiveDate::from_ymd_opt(2026, 11, 23).unwrap(),
            NaiveDate::from_ymd_opt(2026, 11, 30).unwrap(),
        );
        assert!(url.ends_with(
            "/beta/me/settings/workHoursAndLocations/occurrencesView(startDateTime='2026-11-23T00:00:00Z',endDateTime='2026-11-30T00:00:00Z')"
        ));
    }
}
