use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::Deserialize;
use serde_json::{Value, json};
use session::{ApiResponse, Method, PRESENCE, Request, Scope};

use crate::client::{BATCH_SIZE, Graph};
use crate::error::{Error, Result};
use crate::models::{Photo, Presence};
use crate::urls;

pub const MAX_PRESENCE_IDS: usize = 650;
const PHOTO_SCOPE: &str = "User.ReadBasic.All";
const PRESENCE_SERVICE_URL: &str = "https://presence.teams.microsoft.com/v1/presence/getpresence/";
const PRESENCE_SERVICE_SCOPE: &str = "user_impersonation";
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
    fn service_presences_strip_the_mri_prefix() {
        let parsed = parse_service_presences(&json!([
            {"mri": "8:orgid:a", "presence": {"availability": "Away", "activity": "Away"}},
            {"mri": "8:orgid:b"}
        ]));
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].user_id, "a");
    }
}
