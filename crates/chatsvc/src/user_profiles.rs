use serde_json::{Value, json};
use session::{Method, Request, SPACES, Scope, Session};

use crate::error::Result;
use crate::language_settings::with_headers;
use crate::messages::ensure_success;
use crate::pins::{CsaTransport, DEFAULT_REGION, SessionTransport};

const SKYPE_SCOPE: &str = "user_impersonation";
const ORGID_PREFIX: &str = "8:orgid:";
const PROFILE_QUERY: &str =
    "isMailAddress=false&enableGuest=true&includeIBBarredUsers=true&skypeTeamsInfo=true";
const PROFILES_PER_REQUEST: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserProfile {
    pub user_id: String,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub user_principal_name: Option<String>,
    pub job_title: Option<String>,
    pub department: Option<String>,
}

pub struct UserProfiles<T: CsaTransport = SessionTransport> {
    transport: T,
    url: String,
}

impl UserProfiles<SessionTransport> {
    pub fn new(session: &Session) -> Self {
        let transport = SessionTransport::with_scope(session, Scope::new(SPACES, SKYPE_SCOPE));
        UserProfiles::with_transport(transport, DEFAULT_REGION)
    }
}

impl<T: CsaTransport> UserProfiles<T> {
    pub fn with_transport(transport: T, region: &str) -> Self {
        UserProfiles {
            transport,
            url: format!(
                "https://teams.cloud.microsoft/api/mt/{region}/beta/users/fetchShortProfile?{PROFILE_QUERY}"
            ),
        }
    }

    pub async fn fetch(&self, user_ids: &[String]) -> Result<Vec<UserProfile>> {
        let mut profiles = Vec::new();
        for chunk in user_ids.chunks(PROFILES_PER_REQUEST) {
            let mris: Vec<Value> = chunk
                .iter()
                .map(|user_id| json!(format!("{ORGID_PREFIX}{user_id}")))
                .collect();
            let answer = self
                .transport
                .send(with_headers(Request::with_body(
                    Method::Post,
                    self.url.clone(),
                    Value::Array(mris),
                )))
                .await?;
            ensure_success(&answer)?;
            profiles.extend(parse_user_profiles(&answer.body));
        }
        Ok(profiles)
    }
}

pub fn parse_user_profiles(body: &Value) -> Vec<UserProfile> {
    body.get("value")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|profile| {
            let user_id = profile
                .get("objectId")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .or_else(|| {
                    profile
                        .get("mri")
                        .and_then(Value::as_str)?
                        .strip_prefix(ORGID_PREFIX)
                })?
                .to_owned();
            Some(UserProfile {
                user_id,
                display_name: text(profile, "displayName"),
                email: text(profile, "email"),
                user_principal_name: text(profile, "userPrincipalName"),
                job_title: text(profile, "jobTitle"),
                department: text(profile, "department"),
            })
        })
        .collect()
}

fn text(profile: &Value, key: &str) -> Option<String> {
    profile
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use session::ApiResponse;

    use super::*;

    struct Canned {
        requests: Mutex<Vec<Request>>,
    }

    impl CsaTransport for &Canned {
        async fn send(&self, request: Request) -> Result<ApiResponse> {
            self.requests.lock().unwrap().push(request);
            Ok(ApiResponse {
                status: 200,
                body: json!({"value": [
                    {"mri": "8:orgid:ann", "objectId": "ann", "displayName": "Ann Example", "email": "ann@example.com"},
                    {"mri": "8:orgid:bob", "displayName": " "},
                ]}),
                retry_after: None,
            })
        }
    }

    #[tokio::test]
    async fn profiles_post_mris_and_read_names() {
        let canned = Canned {
            requests: Mutex::new(Vec::new()),
        };
        let profiles = UserProfiles::with_transport(&canned, "emea")
            .fetch(&["ann".into(), "bob".into()])
            .await
            .unwrap();
        assert_eq!(profiles[0].display_name.as_deref(), Some("Ann Example"));
        assert_eq!(profiles[0].email.as_deref(), Some("ann@example.com"));
        assert_eq!(profiles[1].user_id, "bob");
        assert_eq!(profiles[1].display_name, None);
        let requests = canned.requests.lock().unwrap();
        assert_eq!(
            requests[0].body,
            Some(json!(["8:orgid:ann", "8:orgid:bob"]))
        );
    }
}
