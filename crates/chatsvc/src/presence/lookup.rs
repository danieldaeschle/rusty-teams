use serde_json::{Value, json};
use session::{Method, PRESENCE, Request, Scope, Session};
use uuid::Uuid;

use crate::error::Result;

pub const MAX_PRESENCE_IDS: usize = 650;
pub const MAX_SUBSCRIBE_IDS: usize = 200;
const PRESENCE_SERVICE_URL: &str = "https://presence.teams.microsoft.com/v1/presence/getpresence/";
const PRESENCE_SERVICE_SCOPE: &str = "user_impersonation";
const PRESENCE_SUBSCRIBE_URL: &str =
    "https://presence.teams.microsoft.com/v1/pubsub/subscriptions/";
const PRESENCE_CLIENT_VERSION: &str = "1415/26091712213";
const ORGID_PREFIX: &str = "8:orgid:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserPresence {
    pub user_id: String,
    pub availability: String,
    pub activity: Option<String>,
}

pub struct PresenceLookup {
    session: Session,
}

impl PresenceLookup {
    pub fn new(session: &Session) -> Self {
        PresenceLookup {
            session: session.clone(),
        }
    }

    /// Graph `Presence.Read.All` is not in the web app tokens.
    pub async fn presences(&self, user_ids: &[String]) -> Result<Vec<UserPresence>> {
        let scope = Scope::new(PRESENCE, PRESENCE_SERVICE_SCOPE);
        let mut presences = Vec::with_capacity(user_ids.len());
        for chunk in user_ids.chunks(MAX_PRESENCE_IDS) {
            let body: Vec<Value> = chunk
                .iter()
                .map(|user_id| json!({"mri": format!("{ORGID_PREFIX}{user_id}")}))
                .collect();
            let answer = self
                .session
                .request(
                    Method::Post,
                    PRESENCE_SERVICE_URL,
                    &scope,
                    Some(Value::Array(body)),
                )
                .await?;
            presences.extend(parse_service_presences(&answer.body));
        }
        Ok(presences)
    }

    pub async fn subscribe(
        &self,
        endpoint_id: &str,
        trouter_uri: &str,
        user_ids: &[String],
        purge: bool,
    ) -> Result<()> {
        let scope = Scope::new(PRESENCE, PRESENCE_SERVICE_SCOPE);
        let url = format!("{PRESENCE_SUBSCRIBE_URL}{endpoint_id}");
        for (index, chunk) in user_ids.chunks(MAX_SUBSCRIBE_IDS).enumerate() {
            let request = Request {
                method: Method::Post,
                headers: subscribe_headers(endpoint_id),
                body: Some(subscribe_body(trouter_uri, chunk, purge && index == 0)),
                ..Request::get(url.clone())
            };
            let answer = self.session.send(request, &scope).await?;
            if !(200..300).contains(&answer.status) {
                return Err(session::Error::api(answer.status, &url, answer.body).into());
            }
        }
        Ok(())
    }
}

// The service answers 400 naming each of the three x-ms-* client headers when one is missing.
fn subscribe_headers(endpoint_id: &str) -> Vec<(String, String)> {
    vec![
        ("x-ms-client-user-agent".into(), "Teams-V2-Web".into()),
        ("x-ms-correlation-id".into(), Uuid::new_v4().to_string()),
        ("x-ms-client-version".into(), PRESENCE_CLIENT_VERSION.into()),
        ("x-ms-endpoint-id".into(), endpoint_id.into()),
        ("x-ms-client-type".into(), "cdlworker".into()),
    ]
}

fn subscribe_body(trouter_uri: &str, user_ids: &[String], purge: bool) -> Value {
    let additions: Vec<Value> = user_ids
        .iter()
        .map(|user_id| json!({"mri": format!("{ORGID_PREFIX}{user_id}"), "source": "ups"}))
        .collect();
    json!({
        "trouterUri": trouter_uri,
        "shouldPurgePreviousSubscriptions": purge,
        "subscriptionsToAdd": additions,
        "subscriptionsToRemove": [],
    })
}

fn parse_service_presences(body: &Value) -> Vec<UserPresence> {
    body.as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let mri = entry.get("mri")?.as_str()?;
            let presence = entry.get("presence")?;
            Some(UserPresence {
                user_id: mri.strip_prefix(ORGID_PREFIX).unwrap_or(mri).to_owned(),
                availability: presence.get("availability")?.as_str()?.to_owned(),
                activity: if presence
                    .pointer("/calendarData/isOutOfOffice")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    Some("OutOfOffice".to_owned())
                } else {
                    presence
                        .get("activity")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                },
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscribe_body_carries_mris_and_purge_flag() {
        let body = subscribe_body("https://t/unifiedPresenceService", &["a".into()], true);
        assert_eq!(
            body,
            json!({
                "trouterUri": "https://t/unifiedPresenceService",
                "shouldPurgePreviousSubscriptions": true,
                "subscriptionsToAdd": [{"mri": "8:orgid:a", "source": "ups"}],
                "subscriptionsToRemove": [],
            })
        );
    }

    #[test]
    fn subscribe_headers_use_a_fresh_correlation_id() {
        let correlation = |headers: &[(String, String)]| {
            headers
                .iter()
                .find(|(name, _)| name == "x-ms-correlation-id")
                .map(|(_, value)| value.clone())
        };
        let first = subscribe_headers("e");
        assert_ne!(correlation(&first), correlation(&subscribe_headers("e")));
        assert!(
            first
                .iter()
                .any(|(name, value)| name == "x-ms-endpoint-id" && value == "e")
        );
    }

    #[test]
    fn service_presences_strip_the_mri_prefix() {
        let parsed = parse_service_presences(&json!([
            {"mri": "8:orgid:a", "presence": {"availability": "Away", "activity": "Away"}},
            {"mri": "8:orgid:b"}
        ]));
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].user_id, "a");
    }

    #[test]
    fn out_of_office_from_the_calendar_becomes_the_activity() {
        let parsed = parse_service_presences(&json!([
            {"mri": "8:orgid:a", "presence": {"availability": "Offline", "activity": "OffWork", "calendarData": {"isOutOfOffice": true}}},
            {"mri": "8:orgid:b", "presence": {"availability": "Available", "activity": "Available", "calendarData": {"isOutOfOffice": false}}}
        ]));
        assert_eq!(parsed[0].activity.as_deref(), Some("OutOfOffice"));
        assert_eq!(parsed[1].activity.as_deref(), Some("Available"));
    }
}
