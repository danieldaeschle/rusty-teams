use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};
use session::{App, Scope, Session};

use crate::error::{Error, Result};
use crate::pins::transport::{CSA_RESOURCE, CSA_SCOPE, skype_fetch_answer};
use crate::messages::ensure_success;

const AMS_BASE: &str = "https://de-prod.asyncgw.teams.microsoft.com/";
const UPLOAD_BODY: &str = concat!(
    include_str!("../assets/skype_token.js"),
    include_str!("../assets/ams_upload.js")
);
const FULL_SIZE_VIEW: &str = "/views/imgpsh_fullsize";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmsImage {
    pub object_id: String,
    pub url: String,
}

pub fn full_size_url(object_id: &str) -> String {
    format!("{AMS_BASE}v1/objects/{object_id}{FULL_SIZE_VIEW}")
}

fn uploaded_image(body: &Value) -> Result<AmsImage> {
    let object_id = body
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && id.chars().all(|character| character.is_ascii_alphanumeric() || character == '-'))
        .ok_or_else(|| Error::UnexpectedAnswer("ams upload has no object id".into()))?;
    Ok(AmsImage {
        object_id: object_id.to_owned(),
        url: full_size_url(object_id),
    })
}

pub struct Ams {
    session: Session,
}

impl Ams {
    pub fn new(session: &Session) -> Self {
        Ams {
            session: session.clone(),
        }
    }

    /// The image is readable by members of `conversation_id` only.
    pub async fn upload_image(&self, conversation_id: &str, bytes: &[u8], filename: &str) -> Result<AmsImage> {
        let arguments = json!({
            "base": AMS_BASE,
            "conversationId": conversation_id,
            "filename": filename,
            "base64": STANDARD.encode(bytes),
        });
        let answer = self
            .session
            .run_with_token(
                App::Teams,
                &Scope::new(CSA_RESOURCE, CSA_SCOPE),
                UPLOAD_BODY,
                &arguments,
                false,
            )
            .await?;
        let answer = skype_fetch_answer(answer);
        ensure_success(&answer)?;
        uploaded_image(&answer.body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_uploaded_object_becomes_its_full_size_view() {
        let image = uploaded_image(&json!({"id": "0-weu-d4-abc123"})).unwrap();
        assert_eq!(image.object_id, "0-weu-d4-abc123");
        assert_eq!(
            image.url,
            "https://de-prod.asyncgw.teams.microsoft.com/v1/objects/0-weu-d4-abc123/views/imgpsh_fullsize"
        );
        assert!(uploaded_image(&json!({"id": "../x"})).is_err());
        assert!(uploaded_image(&json!({})).is_err());
    }
}
