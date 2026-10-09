use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use session::{ApiResponse, Method, Request, Scope};

use crate::client::Graph;
use crate::error::Result;
use crate::urls;

const PROFILE_SCOPE: &str = "User.ReadBasic.All";
const SCHEDULE_SCOPE: &str = "Calendars.Read.Shared";

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct UserProfile {
    pub id: String,
    pub display_name: Option<String>,
    pub job_title: Option<String>,
    pub department: Option<String>,
    pub company_name: Option<String>,
    pub office_location: Option<String>,
    pub mail: Option<String>,
    pub user_principal_name: Option<String>,
    pub business_phones: Vec<String>,
    pub mobile_phone: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct OrgPerson {
    pub id: String,
    pub display_name: Option<String>,
    pub job_title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileDetails {
    pub user: UserProfile,
    pub manager: Option<OrgPerson>,
    pub direct_reports: Vec<OrgPerson>,
}

impl Graph {
    pub async fn user_profile(&self, user_id: &str) -> Result<ProfileDetails> {
        let requests = [
            Request::get(urls::user_profile(user_id)),
            Request::get(urls::user_manager(user_id)),
            Request::get(urls::user_direct_reports(user_id)),
        ];
        let mut answers = self
            .session()
            .batch(&requests, &Scope::graph(PROFILE_SCOPE))
            .await?
            .into_iter();
        let (Some(profile), Some(manager), Some(reports)) =
            (answers.next(), answers.next(), answers.next())
        else {
            return Err(session::Error::api(0, &requests[0].url, Value::Null).into());
        };
        parse_profile_details(&requests[0].url, profile, manager, reports)
    }

    pub async fn time_zone_name(&self, mail: &str) -> Result<Option<String>> {
        let body = schedule_body(mail, Utc::now());
        let answer = self
            .session()
            .send(
                Request::with_body(Method::Post, urls::get_schedule(), body),
                &Scope::graph(SCHEDULE_SCOPE),
            )
            .await?;
        Ok(parse_time_zone_name(&answer.body))
    }
}

fn parse_profile_details(
    url: &str,
    profile: ApiResponse,
    manager: ApiResponse,
    reports: ApiResponse,
) -> Result<ProfileDetails> {
    if !profile.is_success() {
        return Err(session::Error::api(profile.status, url, profile.body).into());
    }
    let manager = manager
        .is_success()
        .then(|| serde_json::from_value(manager.body).ok())
        .flatten();
    let direct_reports = reports
        .is_success()
        .then(|| reports.body.get("value").cloned())
        .flatten()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default();
    Ok(ProfileDetails {
        user: serde_json::from_value(profile.body)?,
        manager,
        direct_reports,
    })
}

fn schedule_body(mail: &str, now: DateTime<Utc>) -> Value {
    let stamp = |moment: DateTime<Utc>| {
        json!({
            "dateTime": moment.to_rfc3339_opts(SecondsFormat::Secs, true).trim_end_matches('Z'),
            "timeZone": "UTC",
        })
    };
    json!({
        "schedules": [mail],
        "startTime": stamp(now),
        "endTime": stamp(now + Duration::hours(1)),
        "availabilityViewInterval": 60,
    })
}

fn parse_time_zone_name(body: &Value) -> Option<String> {
    let name = body
        .pointer("/value/0/workingHours/timeZone/name")?
        .as_str()?
        .trim();
    (!name.is_empty()).then(|| name.to_owned())
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn answer(status: u16, body: Value) -> ApiResponse {
        ApiResponse {
            status,
            body,
            retry_after: None,
        }
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 9, 12, 0, 0).unwrap()
    }

    #[test]
    fn profile_details_parse_fields_manager_and_reports() {
        let profile = json!({
            "id": "u1",
            "displayName": "Ada Lovelace",
            "jobTitle": "Engineer",
            "department": null,
            "mail": "ada@example.com",
            "businessPhones": ["+49 1"],
            "mobilePhone": null
        });
        let manager = json!({"id": "m1", "displayName": "Grace", "jobTitle": "Lead"});
        let reports = json!({"value": [{"@odata.type": "#microsoft.graph.user", "id": "r1", "displayName": "Bob"}]});
        let details = parse_profile_details(
            "u",
            answer(200, profile),
            answer(200, manager),
            answer(200, reports),
        )
        .unwrap();
        assert_eq!(details.user.display_name.as_deref(), Some("Ada Lovelace"));
        assert_eq!(details.user.department, None);
        assert_eq!(details.user.business_phones, vec!["+49 1"]);
        assert_eq!(details.manager.unwrap().id, "m1");
        assert_eq!(
            details.direct_reports[0].display_name.as_deref(),
            Some("Bob")
        );
    }

    #[test]
    fn missing_manager_and_failed_reports_do_not_fail_the_profile() {
        let details = parse_profile_details(
            "u",
            answer(200, json!({"id": "u1"})),
            answer(404, json!({})),
            answer(403, json!({})),
        )
        .unwrap();
        assert!(details.manager.is_none());
        assert!(details.direct_reports.is_empty());
    }

    #[test]
    fn a_failed_profile_is_an_error() {
        let failed = parse_profile_details(
            "u",
            answer(404, json!({})),
            answer(200, json!({"id": "m"})),
            answer(200, json!({"value": []})),
        );
        assert!(failed.is_err());
    }

    #[test]
    fn schedule_body_asks_one_hour_in_utc() {
        let body = schedule_body("ada@example.com", now());
        assert_eq!(body["schedules"], json!(["ada@example.com"]));
        assert_eq!(
            body["startTime"],
            json!({"dateTime": "2026-10-09T12:00:00", "timeZone": "UTC"})
        );
        assert_eq!(body["endTime"]["dateTime"], "2026-10-09T13:00:00");
    }

    #[test]
    fn time_zone_comes_from_working_hours() {
        let body =
            json!({"value": [{"workingHours": {"timeZone": {"name": "W. Europe Standard Time"}}}]});
        assert_eq!(
            parse_time_zone_name(&body).as_deref(),
            Some("W. Europe Standard Time")
        );
        assert_eq!(parse_time_zone_name(&json!({"value": [{}]})), None);
    }
}
