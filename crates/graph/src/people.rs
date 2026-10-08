use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::Deserialize;
use serde_json::{Value, json};
use session::{ApiResponse, Method, PRESENCE, Request, Scope};
use uuid::Uuid;

use crate::client::{BATCH_SIZE, Graph};
use crate::error::{Error, Result};
use crate::models::{Photo, Presence};
use crate::urls;

pub const MAX_PRESENCE_IDS: usize = 650;
pub const MAX_SUBSCRIBE_IDS: usize = 200;
const PHOTO_SCOPE: &str = "User.ReadBasic.All";
const PRESENCE_SERVICE_URL: &str = "https://presence.teams.microsoft.com/v1/presence/getpresence/";
const PRESENCE_SERVICE_SCOPE: &str = "user_impersonation";
const PRESENCE_SUBSCRIBE_URL: &str =
    "https://presence.teams.microsoft.com/v1/pubsub/subscriptions/";
const PRESENCE_CLIENT_VERSION: &str = "1415/26091712213";
const ORGID_PREFIX: &str = "8:orgid:";

#[derive(Debug, Deserialize)]
struct BinaryBody {
    base64: String,
    #[serde(rename = "contentType")]
    content_type: Option<String>,
}

impl Graph {
    /// `Ok(None)` for users without a photo (404).
    pub async fn user_photos(&self, user_ids: &[String]) -> Result<Vec<Result<Option<Photo>>>> {
        let scope = Scope::graph(PHOTO_SCOPE);
        let mut photos = Vec::with_capacity(user_ids.len());
        for chunk in user_ids.chunks(BATCH_SIZE) {
            let requests: Vec<Request> = chunk
                .iter()
                .map(|user_id| Request::binary_get(urls::user_photo(user_id)))
                .collect();
            let answers = self.session().batch(&requests, &scope).await?;
            photos.extend(
                requests
                    .iter()
                    .zip(answers)
                    .map(|(request, answer)| parse_photo(&request.url, answer)),
            );
        }
        Ok(photos)
    }

    /// Teams presence service. Graph `Presence.Read.All` is not in the web app tokens.
    pub async fn presences(&self, user_ids: &[String]) -> Result<Vec<Presence>> {
        let scope = Scope::new(PRESENCE, PRESENCE_SERVICE_SCOPE);
        let mut presences = Vec::with_capacity(user_ids.len());
        for chunk in user_ids.chunks(MAX_PRESENCE_IDS) {
            let body: Vec<Value> = chunk
                .iter()
                .map(|user_id| json!({"mri": format!("{ORGID_PREFIX}{user_id}")}))
                .collect();
            let answer = self
                .session()
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

    pub async fn subscribe_presence(
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
            let answer = self.session().send(request, &scope).await?;
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

fn parse_photo(url: &str, answer: ApiResponse) -> Result<Option<Photo>> {
    match answer.status {
        404 => Ok(None),
        _ => decode_binary(url, answer).map(Some),
    }
}

pub(crate) fn decode_binary(url: &str, answer: ApiResponse) -> Result<Photo> {
    match answer.status {
        200..=299 => {
            let body: BinaryBody = serde_json::from_value(answer.body)?;
            let bytes = STANDARD
                .decode(body.base64.as_bytes())
                .map_err(|error| Error::Decode(error.to_string()))?;
            Ok(Photo {
                bytes,
                content_type: body.content_type.unwrap_or_default(),
            })
        }
        status => Err(session::Error::api(status, url, answer.body).into()),
    }
}

fn parse_service_presences(body: &Value) -> Vec<Presence> {
    body.as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let mri = entry.get("mri")?.as_str()?;
            let presence = entry.get("presence")?;
            Some(Presence {
                user_id: mri.strip_prefix(ORGID_PREFIX).unwrap_or(mri).to_owned(),
                availability: presence.get("availability")?.as_str()?.to_owned(),
                activity: presence
                    .get("activity")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(status: u16, body: Value) -> ApiResponse {
        ApiResponse {
            status,
            body,
            retry_after: None,
        }
    }

    #[test]
    fn photo_decodes_base64_and_content_type() {
        let body = json!({"base64": STANDARD.encode([1u8, 2, 3]), "contentType": "image/jpeg"});
        let photo = parse_photo("u", answer(200, body)).unwrap().unwrap();
        assert_eq!(photo.bytes, [1, 2, 3]);
        assert_eq!(photo.content_type, "image/jpeg");
    }

    #[test]
    fn missing_photo_is_none_and_other_errors_surface() {
        assert!(parse_photo("u", answer(404, json!("x"))).unwrap().is_none());
        assert!(parse_photo("u", answer(403, json!("x"))).is_err());
    }

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
}
