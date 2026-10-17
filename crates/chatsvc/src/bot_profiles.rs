use serde_json::{Value, json};
use session::{Method, Request, SPACES, Scope, Session};

use crate::error::Result;
use crate::language_settings::with_headers;
use crate::messages::ensure_success;
use crate::pins::{CsaTransport, DEFAULT_REGION, SessionTransport};

const SKYPE_SCOPE: &str = "user_impersonation";
const BOT_MRI_PREFIX: &str = "28:";
const BOT_TYPE: &str = "BOT";
const PLACEHOLDER_IMAGE_HOST: &str = "www.contoso.com";
const SECURE_SCHEME: &str = "https://";
const PROFILE_QUERY: &str = "isMailAddress=false&enableGuest=true&includeIBBarredUsers=true&skypeTeamsInfo=true&includeBots=true";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotProfile {
    pub bot_id: String,
    pub display_name: String,
    pub image_url: Option<String>,
}

pub struct BotProfiles<T: CsaTransport = SessionTransport> {
    transport: T,
    url: String,
}

impl BotProfiles<SessionTransport> {
    pub fn new(session: &Session) -> Self {
        let transport = SessionTransport::with_scope(session, Scope::new(SPACES, SKYPE_SCOPE));
        BotProfiles::with_transport(transport, DEFAULT_REGION)
    }
}

impl<T: CsaTransport> BotProfiles<T> {
    pub fn with_transport(transport: T, region: &str) -> Self {
        BotProfiles {
            transport,
            url: format!(
                "https://teams.cloud.microsoft/api/mt/{region}/beta/users/fetchShortProfile?{PROFILE_QUERY}"
            ),
        }
    }

    pub async fn fetch(&self, bot_ids: &[String]) -> Result<Vec<BotProfile>> {
        if bot_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mris: Vec<Value> = bot_ids
            .iter()
            .map(|bot_id| json!(format!("{BOT_MRI_PREFIX}{bot_id}")))
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
        Ok(parse_bot_profiles(&answer.body))
    }
}

pub fn parse_bot_profiles(body: &Value) -> Vec<BotProfile> {
    body.get("value")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter(|profile| profile.get("type").and_then(Value::as_str) == Some(BOT_TYPE))
        .filter_map(|profile| {
            let bot_id = profile
                .get("mri")
                .and_then(Value::as_str)?
                .strip_prefix(BOT_MRI_PREFIX)?
                .to_owned();
            let display_name = profile
                .get("displayName")
                .and_then(Value::as_str)
                .filter(|name| !name.trim().is_empty())?
                .to_owned();
            let image_url = profile
                .get("imageUri")
                .and_then(Value::as_str)
                .filter(|url| is_real_image(url))
                .map(str::to_owned);
            Some(BotProfile {
                bot_id,
                display_name,
                image_url,
            })
        })
        .collect()
}

fn is_real_image(url: &str) -> bool {
    url.strip_prefix(SECURE_SCHEME)
        .and_then(|rest| rest.split('/').next())
        .is_some_and(|host| !host.is_empty() && host != PLACEHOLDER_IMAGE_HOST)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use session::ApiResponse;

    use super::*;

    struct Canned {
        body: Value,
        requests: Mutex<Vec<Request>>,
    }

    impl CsaTransport for &Canned {
        async fn send(&self, request: Request) -> Result<ApiResponse> {
            self.requests.lock().unwrap().push(request);
            Ok(ApiResponse {
                status: 200,
                body: self.body.clone(),
                retry_after: None,
            })
        }
    }

    fn answer() -> Value {
        json!({"value": [
            {"mri": "28:cf28171e", "displayName": "Echo", "type": "BOT", "imageUri": "http://www.contoso.com/"},
            {"mri": "28:358f0194", "displayName": "Workflows", "type": "BOT",
             "imageUri": "https://res.cdn.office.net/teamsappdata/workflows_largeimage.png"},
            {"mri": "8:orgid:user-ada", "displayName": "Ada Example", "type": "ADUser"}
        ]})
    }

    #[test]
    fn bots_keep_their_name_and_only_a_real_icon() {
        let profiles = parse_bot_profiles(&answer());
        assert_eq!(
            profiles,
            [
                BotProfile {
                    bot_id: "cf28171e".into(),
                    display_name: "Echo".into(),
                    image_url: None,
                },
                BotProfile {
                    bot_id: "358f0194".into(),
                    display_name: "Workflows".into(),
                    image_url: Some(
                        "https://res.cdn.office.net/teamsappdata/workflows_largeimage.png".into()
                    ),
                },
            ]
        );
    }

    #[tokio::test]
    async fn the_lookup_posts_bot_mris_to_the_middle_tier() {
        let canned = Canned {
            body: answer(),
            requests: Mutex::new(Vec::new()),
        };
        let client = BotProfiles::with_transport(&canned, "emea");
        let profiles = client.fetch(&["cf28171e".to_owned()]).await.unwrap();
        assert_eq!(profiles.len(), 2);
        let requests = canned.requests.lock().unwrap();
        assert!(requests[0].url.starts_with(
            "https://teams.cloud.microsoft/api/mt/emea/beta/users/fetchShortProfile?"
        ));
        assert_eq!(requests[0].body, Some(json!(["28:cf28171e"])));
    }
}
