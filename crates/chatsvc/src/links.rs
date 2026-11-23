use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use serde_json::{Value, json};
use session::{Method, Request};

use crate::error::{Error, Result};
use crate::messages::{ConversationRef, MessageTransport, Messages, encode, ensure_success};

const URLP_BASE: &str = "https://de-prod.asyncgw.teams.microsoft.com/urlp/v1/url";
pub const LINK_PROXY_HOST_SUFFIX: &str = ".asyncgw.teams.microsoft.com";
const LIST_VIEW: &str = "msnp24Equivalent";
const HOSTED_CONTENTS_SEGMENT: &str = "/hostedContents/";
const HOSTED_URL_KEY: &str = ",url=";
const AMS_HOST_SUFFIX: &str = ".asm.skype.com";
const VIEWS_SEGMENT: &str = "/views/";
const FULL_SIZE_VIEW_PREFIX: &str = "imgpsh_fullsize";
/// The view Teams opens in its image viewer; `_anim` keeps animated GIFs moving.
const FULL_SIZE_VIEW: &str = "imgpsh_fullsize_anim";
const AMS_OBJECTS_PATH: &str = "/v1/objects/";

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

/// The Teams link-preview proxy, the only host a preview image request may carry the token to.
pub fn is_link_image_url(url: &str) -> bool {
    session::https_url(url).is_some_and(|parsed| {
        parsed.port().is_none()
            && parsed
                .host_str()
                .is_some_and(|host| host.ends_with(LINK_PROXY_HOST_SUFFIX))
    })
}

/// The AMS image behind a Graph chat `hostedContents/{id}/$value` link: the id encodes `id=..,type=..,url=<AMS url>`.
pub fn hosted_content_image_url(graph_url: &str) -> Option<String> {
    let after = graph_url.split(HOSTED_CONTENTS_SEGMENT).nth(1)?;
    let encoded = after.split('/').next()?;
    let bytes = STANDARD
        .decode(encoded)
        .or_else(|_| URL_SAFE_NO_PAD.decode(encoded))
        .ok()?;
    let decoded = String::from_utf8(bytes).ok()?;
    let (_, url) = decoded.split_once(HOSTED_URL_KEY)?;
    (is_link_image_url(url) || is_ams_image_url(url)).then(|| url.to_owned())
}

/// An AMS object view (`https://*.asm.skype.com/v1/objects/...`), the inline images of chat service messages.
pub fn is_ams_image_url(url: &str) -> bool {
    session::https_url(url).is_some_and(|parsed| {
        parsed.port().is_none()
            && parsed.path().starts_with(AMS_OBJECTS_PATH)
            && parsed
                .host_str()
                .is_some_and(|host| host.ends_with(AMS_HOST_SUFFIX))
    })
}

/// The full-resolution view of the AMS image at `url` (a plain AMS view or a Graph `hostedContents` link to one).
/// Messages carry the compressed `imgo` view; Teams swaps to this view when the image is opened.
/// `None` for other images, video clips and thumbnails, and views that already are full size.
pub fn ams_full_size_url(url: &str) -> Option<String> {
    let decoded = hosted_content_image_url(url);
    let url = decoded.as_deref().unwrap_or(url);
    let is_object = |host: &str, path: &str| {
        path.starts_with("v1/objects/")
            && (host.ends_with(AMS_HOST_SUFFIX) || host.ends_with(LINK_PROXY_HOST_SUFFIX))
    };
    let (host, path) = url.strip_prefix("https://")?.split_once('/')?;
    if !is_object(host, path) {
        return None;
    }
    let (object, view) = url.rsplit_once(VIEWS_SEGMENT)?;
    let still_picture = view.starts_with("img") && !view.contains(['/', '?', '#']);
    (still_picture && !view.starts_with(FULL_SIZE_VIEW_PREFIX))
        .then(|| format!("{object}{VIEWS_SEGMENT}{FULL_SIZE_VIEW}"))
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
        decoded_image(&answer.body)
    }
}

/// An image answer shaped `{base64, contentType}`.
pub(crate) fn decoded_image(body: &Value) -> Result<LinkImage> {
    let encoded = body
        .get("base64")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let bytes = STANDARD
        .decode(encoded.as_bytes())
        .map_err(|error| Error::UnexpectedAnswer(error.to_string()))?;
    let content_type = body
        .get("contentType")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    Ok(LinkImage {
        bytes,
        content_type,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosted_content_ids_carry_the_ams_url() {
        let ams = "https://de-prod.asyncgw.teams.microsoft.com/v1/objects/0-weu-d1-abc/views/imgo";
        let id = STANDARD.encode(format!("id=x_0-weu-d1-abc,type=1,url={ams}"));
        let graph = format!(
            "https://graph.microsoft.com/v1.0/chats/19:a/messages/1/hostedContents/{id}/$value"
        );
        assert_eq!(hosted_content_image_url(&graph).as_deref(), Some(ams));
        let foreign = STANDARD.encode("id=x,type=1,url=https://evil.example/a");
        assert_eq!(
            hosted_content_image_url(&format!(
                "https://graph.microsoft.com/v1.0/chats/c/messages/1/hostedContents/{foreign}/$value"
            )),
            None
        );
    }

    #[test]
    fn ams_object_views_are_recognized_inside_hosted_content_ids() {
        let ams = "https://eu-api.asm.skype.com/v1/objects/0-frca-d14-abc/views/imgo";
        assert!(is_ams_image_url(ams));
        let id = STANDARD.encode(format!("id=x_0-frca-d14-abc,type=1,url={ams}"));
        assert_eq!(
            hosted_content_image_url(&format!(
                "https://graph.microsoft.com/v1.0/teams/t/channels/c/messages/1/hostedContents/{id}/$value"
            ))
            .as_deref(),
            Some(ams)
        );
        assert!(!is_ams_image_url(
            "https://evil.example/v1/objects/a/views/imgo"
        ));
        assert!(!is_ams_image_url(
            "https://eu-api.asm.skype.com/v1/users/me"
        ));
    }

    #[test]
    fn an_inline_ams_image_opens_in_its_full_size_view() {
        assert_eq!(
            ams_full_size_url("https://eu-api.asm.skype.com/v1/objects/0-weu-d1-abc/views/imgo")
                .as_deref(),
            Some("https://eu-api.asm.skype.com/v1/objects/0-weu-d1-abc/views/imgpsh_fullsize_anim")
        );
        assert_eq!(
            ams_full_size_url(
                "https://de-prod.asyncgw.teams.microsoft.com/v1/objects/0-weu-d4-abc/views/imgo"
            )
            .as_deref(),
            Some(
                "https://de-prod.asyncgw.teams.microsoft.com/v1/objects/0-weu-d4-abc/views/imgpsh_fullsize_anim"
            )
        );
        let ams = "https://eu-api.asm.skype.com/v1/objects/0-frca-d14-abc/views/imgo";
        let id = STANDARD.encode(format!("id=x_0-frca-d14-abc,type=1,url={ams}"));
        assert_eq!(
            ams_full_size_url(&format!(
                "https://graph.microsoft.com/v1.0/chats/19:a/messages/1/hostedContents/{id}/$value"
            ))
            .as_deref(),
            Some(
                "https://eu-api.asm.skype.com/v1/objects/0-frca-d14-abc/views/imgpsh_fullsize_anim"
            )
        );
    }

    #[test]
    fn full_size_and_non_picture_views_have_no_separate_full_size() {
        for url in [
            "https://eu-api.asm.skype.com/v1/objects/a/views/imgpsh_fullsize",
            "https://eu-api.asm.skype.com/v1/objects/a/views/imgpsh_fullsize_anim",
            "https://eu-api.asm.skype.com/v1/objects/a/views/video",
            "https://eu-api.asm.skype.com/v1/objects/a/views/thumbnail",
            "https://eu-api.asm.skype.com/v1/objects/a",
            "https://evil.example/v1/objects/a/views/imgo",
            "https://de-prod.asyncgw.teams.microsoft.com/urlp/v1/url/content?url=x/views/imgo",
            "https://graph.microsoft.com/v1.0/chats/c/messages/1/hostedContents/aGk=/$value",
        ] {
            assert_eq!(ams_full_size_url(url), None, "{url}");
        }
    }

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

    /// Regression: the HTTP client reads `\` as a path separator, so these went to evil.com with the token.
    #[test]
    fn a_backslash_or_user_info_does_not_pass_as_a_proxy_host() {
        assert!(!is_link_image_url(
            "https://evil.com\\@x.asyncgw.teams.microsoft.com/urlp/v1/url/image"
        ));
        assert!(!is_link_image_url(
            "https://evil.com#.asyncgw.teams.microsoft.com/urlp/v1/url/image"
        ));
        assert!(!is_link_image_url(
            "https://user@de-prod.asyncgw.teams.microsoft.com/urlp/v1/url/image"
        ));
        assert!(!is_link_image_url(
            "https://de-prod.asyncgw.teams.microsoft.com:8443/urlp/v1/url/image"
        ));
    }

    /// Regression: `?` ended the hand-split host early, so this went to evil.com with the Skype token.
    #[test]
    fn a_query_or_backslash_does_not_pass_as_an_ams_host() {
        assert!(!is_ams_image_url(
            "https://evil.com?.asm.skype.com/v1/objects/x"
        ));
        assert!(!is_ams_image_url(
            "https://evil.com\\@eu-api.asm.skype.com/v1/objects/x/views/imgo"
        ));
        assert_eq!(
            hosted_content_image_url(&format!(
                "https://graph.microsoft.com/v1.0/chats/c/messages/1/hostedContents/{}/$value",
                STANDARD.encode("id=x,type=1,url=https://evil.com?.asm.skype.com/v1/objects/x")
            )),
            None
        );
    }
}
