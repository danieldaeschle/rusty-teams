use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};
use session::{Method, Request};

use crate::error::{Error, Result};
use crate::messages::{ConversationRef, MessageTransport, Messages, encode, ensure_success};

const URLP_BASE: &str = "https://de-prod.asyncgw.teams.microsoft.com/urlp/v1/url";
const IMAGE_HOST_SUFFIX: &str = ".asyncgw.teams.microsoft.com";
const LIST_VIEW: &str = "msnp24Equivalent";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageLinks {
    pub message_id: String,
    pub links_json: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LinkInfo {
    pub url: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub site: Option<String>,
    pub thumbnail: Option<String>,
    pub thumbnail_width: Option<u32>,
    pub thumbnail_height: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkImage {
    pub bytes: Vec<u8>,
    pub content_type: String,
}

pub fn is_link_image_url(url: &str) -> bool {
    url.strip_prefix("https://")
        .and_then(|rest| rest.split(['/', '?', '#']).next())
        .is_some_and(|host| host.ends_with(IMAGE_HOST_SUFFIX))
}

pub fn parse_message_links(body: &Value) -> Vec<MessageLinks> {
    body.get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|message| {
            let message_id = message.get("id")?.as_str()?.to_owned();
            let links_json = match message.pointer("/properties/links")? {
                Value::String(text) => text.clone(),
                array @ Value::Array(_) => array.to_string(),
                _ => return None,
            };
            Some(MessageLinks {
                message_id,
                links_json,
            })
        })
        .collect()
}

pub fn parse_link_info(body: &Value) -> LinkInfo {
    let text = |key: &str| {
        body.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    let size = |key: &str| {
        body.pointer(&format!("/thumbnail_meta/{key}"))
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value > 0)
    };
    LinkInfo {
        url: text("target_url")
            .or_else(|| text("url"))
            .unwrap_or_default(),
        title: text("title"),
        description: text("description"),
        site: text("site"),
        thumbnail: text("thumbnail"),
        thumbnail_width: size("width"),
        thumbnail_height: size("height"),
    }
}

impl<T: MessageTransport> Messages<T> {
    pub async fn list_message_links(
        &self,
        conversation: &ConversationRef,
        page_size: usize,
    ) -> Result<Vec<MessageLinks>> {
        let url = format!(
            "{}/{}/messages?view={LIST_VIEW}&pageSize={page_size}",
            self.base_url,
            encode(&conversation.conversation_id())
        );
        let answer = self.transport.send(Request::get(&url)).await?;
        ensure_success(&answer)?;
        Ok(parse_message_links(&answer.body))
    }

    pub async fn set_links(
        &self,
        conversation: &ConversationRef,
        message_id: &str,
        links_json: &str,
    ) -> Result<()> {
        let url = format!(
            "{}/properties?name=links",
            self.message_url(conversation, message_id)
        );
        let body = json!({"links": links_json});
        let answer = self
            .transport
            .send(Request::with_body(Method::Put, url, body))
            .await?;
        ensure_success(&answer)
    }

    pub async fn link_info(&self, url: &str) -> Result<LinkInfo> {
        let request_url = format!("{URLP_BASE}/info?url={}", encode(url));
        let answer = self.transport.send(Request::get(&request_url)).await?;
        ensure_success(&answer)?;
        Ok(parse_link_info(&answer.body))
    }

    pub async fn link_image(&self, url: &str) -> Result<LinkImage> {
        if !is_link_image_url(url) {
            return Err(Error::UnexpectedAnswer(format!(
                "not a link preview image url: {url}"
            )));
        }
        let answer = self.transport.send(Request::binary_get(url)).await?;
        ensure_success(&answer)?;
        let encoded = answer
            .body
            .get("base64")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let bytes = STANDARD
            .decode(encoded.as_bytes())
            .map_err(|error| Error::UnexpectedAnswer(error.to_string()))?;
        let content_type = answer
            .body
            .get("contentType")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        Ok(LinkImage {
            bytes,
            content_type,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_link_info_with_target_url_and_thumbnail_size() {
        let info = parse_link_info(&json!({
            "title": " Rust ",
            "description": "",
            "site": "GitHub",
            "thumbnail": "https://de-prod.asyncgw.teams.microsoft.com/urlp/v1/url/image/Thumbnail?url=x",
            "thumbnail_meta": {"height": 160, "width": 320},
            "url": "https://github.com/a",
            "target_url": "https://github.com/rust-lang/rust",
        }));
        assert_eq!(info.url, "https://github.com/rust-lang/rust");
        assert_eq!(info.title.as_deref(), Some("Rust"));
        assert_eq!(info.description, None);
        assert_eq!(info.site.as_deref(), Some("GitHub"));
        assert_eq!(
            (info.thumbnail_width, info.thumbnail_height),
            (Some(320), Some(160))
        );
    }

    #[test]
    fn link_info_tolerates_missing_fields() {
        let info = parse_link_info(&json!({"url": "https://a.example", "thumbnail_meta": null}));
        assert_eq!(info.url, "https://a.example");
        assert_eq!(info.title, None);
        assert_eq!(info.thumbnail, None);
        assert_eq!(info.thumbnail_width, None);
    }

    #[test]
    fn reads_links_as_string_or_array_and_skips_messages_without() {
        let links = parse_message_links(&json!({"messages": [
            {"id": "1", "properties": {"links": "[{\"url\":\"https://a\"}]"}},
            {"id": "2", "properties": {"links": [{"url": "https://b"}]}},
            {"id": "3", "properties": {}},
            {"id": "4"},
            {"properties": {"links": "[]"}},
        ]}));
        assert_eq!(
            links,
            vec![
                MessageLinks {
                    message_id: "1".into(),
                    links_json: "[{\"url\":\"https://a\"}]".into()
                },
                MessageLinks {
                    message_id: "2".into(),
                    links_json: "[{\"url\":\"https://b\"}]".into()
                },
            ]
        );
    }

    #[test]
    fn only_asyncgw_hosts_receive_the_token() {
        assert!(is_link_image_url(
            "https://de-prod.asyncgw.teams.microsoft.com/urlp/v1/url/image/Thumbnail?url=x"
        ));
        assert!(!is_link_image_url(
            "https://evil.example/?a=.asyncgw.teams.microsoft.com"
        ));
        assert!(!is_link_image_url(
            "http://de-prod.asyncgw.teams.microsoft.com/x"
        ));
    }
}
