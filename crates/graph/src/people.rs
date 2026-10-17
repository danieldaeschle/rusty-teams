use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::Deserialize;
use session::{ApiResponse, Request, Scope};

use crate::client::{BATCH_SIZE, Graph};
use crate::error::{Error, Result};
use crate::models::Photo;
use crate::urls;

const PHOTO_SCOPE: &str = "User.ReadBasic.All";

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

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

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
}
