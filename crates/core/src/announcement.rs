use serde_json::{Value, json};

use crate::stored::{AttachmentInfo, ImageRef};

pub const ANNOUNCEMENT_BANNER_TYPE: &str =
    "application/vnd.microsoft.teams.messaging-announcementBanner";
pub const ANNOUNCEMENT_APP_ID: &str = "announcement-card";
const DEFAULT_BANNER_COLOR: &str = "periwinkleBlue";
const COLOR_THEME: &str = "colorTheme";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BannerBackground {
    Color(String),
    Image(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnouncementBanner {
    pub title: String,
    pub background: BannerBackground,
}

impl AnnouncementBanner {
    pub fn from_attachments(attachments: &[AttachmentInfo]) -> Option<AnnouncementBanner> {
        attachments
            .iter()
            .filter(|attachment| {
                attachment.content_type.as_deref() == Some(ANNOUNCEMENT_BANNER_TYPE)
            })
            .find_map(|attachment| AnnouncementBanner::parse(attachment.content.as_deref()?))
    }

    pub fn parse(content: &str) -> Option<AnnouncementBanner> {
        let content: Value = serde_json::from_str(content).ok()?;
        let title = content.get("title")?.as_str()?.trim().to_owned();
        let details = content.get("cardImageDetails");
        let image = details
            .filter(|_| content.get("cardImageType").and_then(Value::as_str) != Some(COLOR_THEME))
            .and_then(|details| {
                image_source(details, "croppedImage")
                    .or_else(|| image_source(details, "originalImage"))
            });
        let background = match image {
            Some(url) => BannerBackground::Image(url),
            None => BannerBackground::Color(
                details
                    .and_then(|details| details.get(COLOR_THEME))
                    .and_then(Value::as_str)
                    .unwrap_or(DEFAULT_BANNER_COLOR)
                    .to_owned(),
            ),
        };
        Some(AnnouncementBanner { title, background })
    }

    pub fn image(&self) -> Option<ImageRef> {
        let BannerBackground::Image(url) = &self.background else {
            return None;
        };
        Some(ImageRef {
            id: url.clone(),
            url: url.clone(),
            width: None,
            height: None,
            alt: Some(self.title.clone()),
        })
    }

    /// `id` must be unique per message; Teams uses `announcement-card-<guid>`.
    pub fn attachment(&self, id: String) -> graph::KeptAttachment {
        graph::KeptAttachment {
            id,
            content_type: Some(ANNOUNCEMENT_BANNER_TYPE.to_owned()),
            content: Some(self.content()),
            content_url: None,
            name: None,
            teams_app_id: Some(ANNOUNCEMENT_APP_ID.to_owned()),
        }
    }

    /// Only color banners can be sent; uploaded images need the Teams image service.
    pub fn content(&self) -> String {
        let color = match &self.background {
            BannerBackground::Color(color) => color.as_str(),
            BannerBackground::Image(_) => DEFAULT_BANNER_COLOR,
        };
        json!({
            "title": self.title,
            "cardImageType": COLOR_THEME,
            "cardImageDetails": { COLOR_THEME: color },
        })
        .to_string()
    }
}

fn image_source(details: &Value, key: &str) -> Option<String> {
    let found = details
        .get(key)
        .or_else(|| details.get("uploadedImageDetail")?.get(key))?;
    found
        .get("source")
        .or_else(|| found.get("url"))
        .and_then(Value::as_str)
        .filter(|url| url.starts_with("https://"))
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn banner_attachment(content: &str) -> AttachmentInfo {
        serde_json::from_value(json!({
            "content_type": ANNOUNCEMENT_BANNER_TYPE,
            "name": null,
            "url": null,
            "text": null,
            "content": content,
        }))
        .unwrap()
    }

    #[test]
    fn color_banner_from_graph_content() {
        let content = r#"{"title":"Release day","cardImageType":"colorTheme","cardImageDetails":{"colorTheme":"periwinkleBlue"}}"#;
        let banner = AnnouncementBanner::from_attachments(&[banner_attachment(content)]).unwrap();
        assert_eq!(banner.title, "Release day");
        assert_eq!(
            banner.background,
            BannerBackground::Color("periwinkleBlue".into())
        );
    }

    #[test]
    fn uploaded_image_banner_uses_the_cropped_image() {
        let content = r#"{"title":"Hi","cardImageType":"uploadedImage","cardImageDetails":{"uploadedImageDetail":{"originalImage":{"source":"https://example.test/o"},"croppedImage":{"source":"https://example.test/c"}}}}"#;
        let banner = AnnouncementBanner::parse(content).unwrap();
        assert_eq!(
            banner.background,
            BannerBackground::Image("https://example.test/c".into())
        );
    }

    #[test]
    fn content_round_trips() {
        let banner = AnnouncementBanner {
            title: "Ship it".into(),
            background: BannerBackground::Color("teal".into()),
        };
        assert_eq!(AnnouncementBanner::parse(&banner.content()), Some(banner));
    }

    #[test]
    fn banner_without_content_is_none() {
        let mut attachment = banner_attachment("");
        attachment.content = None;
        assert_eq!(AnnouncementBanner::from_attachments(&[attachment]), None);
    }
}
