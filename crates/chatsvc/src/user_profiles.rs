use serde_json::{Value, json};
use session::{Method, Request, SPACES, Scope, Session};

use crate::error::Result;
use crate::language_settings::with_headers;
use crate::messages::{encode, ensure_success};
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
    pub tenant_name: Option<String>,
}

pub struct UserProfiles<T: CsaTransport = SessionTransport> {
    transport: T,
    users_url: String,
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
            users_url: format!("https://teams.cloud.microsoft/api/mt/{region}/beta/users"),
        }
    }

    pub async fn fetch(&self, user_ids: &[String]) -> Result<Vec<UserProfile>> {
        self.post_mris(
            &format!("{}/fetchShortProfile?{PROFILE_QUERY}", self.users_url),
            user_ids,
        )
        .await
    }

    /// People of other organizations; the short profile lookup answers nothing for them.
    pub async fn fetch_federated(&self, user_ids: &[String]) -> Result<Vec<UserProfile>> {
        self.post_mris(&format!("{}/fetchFederated", self.users_url), user_ids)
            .await
    }

    /// Teams finds a person of another organization only by the full email address.
    pub async fn search_external(&self, email: &str) -> Result<Vec<UserProfile>> {
        let answer = self
            .transport
            .send(with_headers(Request::get(format!(
                "{}/{}/externalsearchv3?includeTFLUsers=true&includeEDUsers=true",
                self.users_url,
                encode(email.trim())
            ))))
            .await?;
        ensure_success(&answer)?;
        Ok(parse_user_profiles(&answer.body))
    }

    async fn post_mris(&self, url: &str, user_ids: &[String]) -> Result<Vec<UserProfile>> {
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
                    url.to_owned(),
                    Value::Array(mris),
                )))
                .await?;
            ensure_success(&answer)?;
            profiles.extend(parse_user_profiles(&answer.body));
        }
        Ok(profiles)
    }
}

fn profile_items(body: &Value) -> Vec<&Value> {
    match body {
        Value::Array(items) => items.iter().collect(),
        Value::Object(_) => {
            if let Some(items) = body.get("value").and_then(Value::as_array) {
                return items.iter().collect();
            }
            if let Some(profile) = body.get("value").filter(|value| value.is_object()) {
                return vec![profile];
            }
            if body.get("mri").is_some() || body.get("objectId").is_some() {
                return vec![body];
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

pub fn parse_user_profiles(body: &Value) -> Vec<UserProfile> {
    profile_items(body)
        .into_iter()
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
                email: text(profile, "email")
                    .or_else(|| text(profile, "mail"))
                    .or_else(|| text(profile, "userPrincipalName")),
                user_principal_name: text(profile, "userPrincipalName"),
                job_title: text(profile, "jobTitle"),
                department: text(profile, "department"),
                tenant_name: text(profile, "tenantName"),
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
        assert!(requests[0].url.contains("/users/fetchShortProfile?"));
    }

    #[tokio::test]
    async fn people_of_other_organizations_come_from_the_federated_lookup() {
        let canned = Canned {
            requests: Mutex::new(Vec::new()),
        };
        UserProfiles::with_transport(&canned, "emea")
            .fetch_federated(&["ann".into()])
            .await
            .unwrap();
        let requests = canned.requests.lock().unwrap();
        assert!(requests[0].url.ends_with("/emea/beta/users/fetchFederated"));
        assert_eq!(requests[0].body, Some(json!(["8:orgid:ann"])));
    }

    #[tokio::test]
    async fn external_search_includes_federated_work_accounts() {
        let canned = Canned {
            requests: Mutex::new(Vec::new()),
        };
        UserProfiles::with_transport(&canned, "emea")
            .search_external("ada@other.example")
            .await
            .unwrap();
        let requests = canned.requests.lock().unwrap();
        assert!(requests[0].url.contains("includeTFLUsers=true"));
        assert!(requests[0].url.contains("includeEDUsers=true"));
        assert!(
            requests[0].url.contains("ada%40other.example")
                || requests[0].url.contains("ada@other.example")
        );
    }

    #[test]
    fn an_external_profile_reads_mail_or_the_principal_name() {
        let from_mail = parse_user_profiles(&json!(
            {"mri": "8:orgid:ada", "displayName": "Ada", "mail": "ada@other.example"}
        ));
        assert_eq!(from_mail[0].email.as_deref(), Some("ada@other.example"));
        let from_upn = parse_user_profiles(&json!({"value": {
            "objectId": "bea",
            "displayName": "Bea",
            "userPrincipalName": "bea@other.example"
        }}));
        assert_eq!(from_upn[0].user_id, "bea");
        assert_eq!(from_upn[0].email.as_deref(), Some("bea@other.example"));
    }

    #[test]
    fn a_federated_profile_carries_its_organization() {
        let profiles = parse_user_profiles(&json!({"value": [
            {"mri": "8:orgid:ann", "objectId": "ann", "displayName": "Ann", "tenantName": "Example Corp"},
        ]}));
        assert_eq!(profiles[0].tenant_name.as_deref(), Some("Example Corp"));
    }
}
