use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use serde_json::{Value, json};
use session::{GRAPH, Method, Request, Scope};

use super::Calendar;
use super::schedule::CALENDARS_SCOPE;
use crate::cards::CardTransport;
use crate::error::{Error, Result};

const READ_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.f";
/// Outlook's own GraphQL gateway; Graph serves only the signed-in user's work plan.
const GATEWAY_URL: &str = "https://outlook.office.com/outlookgatewayb2/graphql";
const GATEWAY_RESOURCE: &str = "https://outlook.office.com";
const PEOPLE_URL: &str = "https://graph.microsoft.com/v1.0/me/people?$top=40&$select=displayName,scoredEmailAddresses,personType";
const COLLEAGUES_QUERY: &str = "query GetScheduleWithLocationFWH($input: GetScheduleInput) { getSchedule(request: $input) { schedules { scheduleId flexibleWorkingHours { start { dateTime } workLocationType } } } }";

/// Where the work plan in Outlook puts the user on a day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkPlanLocation {
    Office,
    Remote,
    TimeOff,
    Unspecified,
}

impl WorkPlanLocation {
    /// Graph writes `timeOff`, the Outlook gateway `TimeOff`.
    fn from_graph(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "office" => WorkPlanLocation::Office,
            "remote" => WorkPlanLocation::Remote,
            "timeoff" => WorkPlanLocation::TimeOff,
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

/// Where a colleague's work plan puts them on one day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColleagueLocation {
    pub name: String,
    pub date: NaiveDate,
    pub location: WorkPlanLocation,
}

/// People in the user's organization they work with most, as (name, mail), without the user.
pub fn parse_colleagues(body: &Value, own_mail: &str) -> Vec<(String, String)> {
    body["value"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|person| person["personType"]["subclass"] == "OrganizationUser")
        .filter_map(|person| {
            let mail = person["scoredEmailAddresses"][0]["address"].as_str()?;
            let name = person["displayName"].as_str().unwrap_or(mail);
            (!mail.eq_ignore_ascii_case(own_mail)).then(|| (name.to_owned(), mail.to_owned()))
        })
        .collect()
}

fn colleagues_body(own_mail: &str, mails: &[&str], from: NaiveDate, to: NaiveDate) -> Value {
    let zone = json!({"name": "UTC"});
    json!([{
        "operationName": "GetScheduleWithLocationFWH",
        "query": COLLEAGUES_QUERY,
        "variables": {"input": {
            "userId": own_mail,
            "availabilityViewInterval": 30,
            "startTime": {"dateTime": format!("{from}T00:00:00.000Z"), "timeZone": zone},
            "endTime": {"dateTime": format!("{to}T00:00:00.000Z"), "timeZone": zone},
            "schedules": mails,
            "viewType": "WorkPlansOnly"
        }}
    }])
}

/// Office and remote days of each colleague; unplanned days and unknown mailboxes are left out.
pub fn parse_colleague_locations(
    body: &Value,
    colleagues: &[(String, String)],
) -> Vec<ColleagueLocation> {
    let mut found = Vec::new();
    for schedule in body[0]["data"]["getSchedule"]["schedules"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let Some(name) = schedule["scheduleId"].as_str().and_then(|mail| {
            colleagues
                .iter()
                .find(|(_, known)| known.eq_ignore_ascii_case(mail))
                .map(|(name, _)| name)
        }) else {
            continue;
        };
        for segment in schedule["flexibleWorkingHours"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let location = WorkPlanLocation::from_graph(
                segment["workLocationType"].as_str().unwrap_or_default(),
            );
            let date = segment["start"]["dateTime"]
                .as_str()
                .and_then(|start| start.get(..10))
                .and_then(|day| NaiveDate::parse_from_str(day, "%Y-%m-%d").ok());
            let entry = date.map(|date| ColleagueLocation {
                name: name.clone(),
                date,
                location,
            });
            if let Some(entry) = entry.filter(|entry| {
                matches!(
                    entry.location,
                    WorkPlanLocation::Office | WorkPlanLocation::Remote
                ) && !found.contains(entry)
            }) {
                found.push(entry);
            }
        }
    }
    found
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

    /// Where the user's closest colleagues plan to work on the days from `from` up to `to`.
    pub async fn colleague_locations(
        &self,
        own_mail: &str,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<ColleagueLocation>> {
        let people = self
            .transport
            .send(Request::get(PEOPLE_URL), Scope::graph("People.Read"))
            .await?;
        if !people.is_success() {
            return Err(Error::Session(session::Error::api(
                people.status,
                PEOPLE_URL,
                people.body,
            )));
        }
        let colleagues = parse_colleagues(&people.body, own_mail);
        if colleagues.is_empty() {
            return Ok(Vec::new());
        }
        let mails: Vec<&str> = colleagues.iter().map(|(_, mail)| mail.as_str()).collect();
        let answer = self
            .transport
            .send(
                Request::with_body(
                    Method::Post,
                    GATEWAY_URL,
                    colleagues_body(own_mail, &mails, from, to),
                ),
                Scope::new(GATEWAY_RESOURCE, ".default"),
            )
            .await?;
        if !answer.is_success() {
            return Err(Error::Session(session::Error::api(
                answer.status,
                GATEWAY_URL,
                answer.body,
            )));
        }
        Ok(parse_colleague_locations(&answer.body, &colleagues))
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

    use super::{
        WorkPlanLocation, occurrence_body, parse_colleague_locations, parse_colleagues,
        parse_work_plan, work_plan_url,
    };

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
    fn colleagues_are_org_users_other_than_the_user() {
        let body = json!({"value": [
            {"displayName": "Me", "personType": {"subclass": "OrganizationUser"}, "scoredEmailAddresses": [{"address": "ME@corp.example"}]},
            {"displayName": "Ann", "personType": {"subclass": "OrganizationUser"}, "scoredEmailAddresses": [{"address": "ann@corp.example"}]},
            {"displayName": "Guest", "personType": {"subclass": "PersonalContact"}, "scoredEmailAddresses": [{"address": "g@other.example"}]},
            {"displayName": "No mail", "personType": {"subclass": "OrganizationUser"}, "scoredEmailAddresses": []}
        ]});
        assert_eq!(
            parse_colleagues(&body, "me@corp.example"),
            [("Ann".to_owned(), "ann@corp.example".to_owned())]
        );
    }

    #[test]
    fn colleague_days_keep_office_and_remote_only() {
        let colleagues = [
            ("Ann".to_owned(), "ann@corp.example".to_owned()),
            ("Bob".to_owned(), "bob@corp.example".to_owned()),
        ];
        let body = json!([{"data": {"getSchedule": {"schedules": [
            {"scheduleId": "Ann@corp.example", "flexibleWorkingHours": [
                {"start": {"dateTime": "2026-11-26T08:00:00.000+01:00"}, "workLocationType": "Office"},
                {"start": {"dateTime": "2026-11-26T13:00:00.000+01:00"}, "workLocationType": "Office"},
                {"start": {"dateTime": "2026-11-27T08:00:00.000+01:00"}, "workLocationType": "Remote"}
            ]},
            {"scheduleId": "bob@corp.example", "flexibleWorkingHours": [
                {"start": {"dateTime": "2026-11-26T08:00:00.000+01:00"}, "workLocationType": "Unspecified"}
            ]},
            {"scheduleId": "stranger@corp.example", "flexibleWorkingHours": [
                {"start": {"dateTime": "2026-11-26T08:00:00.000+01:00"}, "workLocationType": "Office"}
            ]},
            {"scheduleId": "bob@corp.example", "error": {"responseCode": "5016"}}
        ]}}}]);
        let found = parse_colleague_locations(&body, &colleagues);
        let day = |d| NaiveDate::from_ymd_opt(2026, 11, d).unwrap();
        let summary: Vec<_> = found
            .iter()
            .map(|c| (c.name.as_str(), c.date, c.location))
            .collect();
        assert_eq!(
            summary,
            [
                ("Ann", day(26), WorkPlanLocation::Office),
                ("Ann", day(27), WorkPlanLocation::Remote)
            ]
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
