use serde_json::{Value, json};

use crate::stored::{AttachmentInfo, ImageRef};

pub const ANNOUNCEMENT_BANNER_TYPE: &str =
    "application/vnd.microsoft.teams.messaging-announcementBanner";
pub const ANNOUNCEMENT_APP_ID: &str = "announcement-card";
const DEFAULT_BANNER_COLOR: &str = "periwinkleBlue";
const COLOR_THEME: &str = "colorTheme";
const UPLOADED_IMAGE: &str = "uploadedImage";
const LOCAL_IMAGE: &str = "localImage";
const TEAMS_THEMES: [&str; 12] = [
    "periwinkleBlue",
    "lavender",
    "rose",
    "orangeSherbet",
    "flaxYellow",
    "teal",
    "navyBlue",
    "indigo",
    "mulberryRed",
    "fireOrange",
    "rawUmber",
    "oceanGreen",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BannerBackground {
    Color(String),
    Image(String),
    /// A picked file that is uploaded when the post is sent.
    LocalImage(String),
}

/// Teams web stores the color theme as its index.
pub fn teams_theme(color: &str) -> String {
    TEAMS_THEMES
        .iter()
        .position(|known| *known == color)
        .unwrap_or_default()
        .to_string()
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
        if let Some(path) = content.get(LOCAL_IMAGE).and_then(Value::as_str) {
            return Some(AnnouncementBanner {
                title,
                background: BannerBackground::LocalImage(path.to_owned()),
            });
        }
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

    /// Graph shape, as Graph returns it for a post.
    pub fn attachment(&self, id: String) -> graph::Attachment {
        graph::Attachment {
            id: Some(id),
            content_type: Some(ANNOUNCEMENT_BANNER_TYPE.to_owned()),
            content_url: None,
            name: None,
            content: Some(self.content()),
            teams_app_id: Some(ANNOUNCEMENT_APP_ID.to_owned()),
        }
    }

    /// Graph shape; a local image keeps its path so a queued post survives a restart.
    pub fn content(&self) -> String {
        let mut content = json!({ "title": self.title });
        match &self.background {
            BannerBackground::Color(color) => {
                content["cardImageType"] = json!(COLOR_THEME);
                content["cardImageDetails"] = json!({ COLOR_THEME: color });
            }
            BannerBackground::Image(url) => {
                content["cardImageType"] = json!(UPLOADED_IMAGE);
                content["cardImageDetails"] = json!({ "uploadedImageDetail": {
                    "originalImage": { "source": url },
                    "croppedImage": { "source": url },
                }});
            }
            BannerBackground::LocalImage(path) => content[LOCAL_IMAGE] = json!(path),
        }
        content.to_string()
    }

    /// The chat service `cards` property Teams web renders; `image` is the uploaded background.
    pub fn teams_cards(&self, image: Option<&TeamsBannerImage>) -> String {
        let content = match (image, &self.background) {
            (Some(image), _) => json!({ "imageData": {
                "origImage": { "id": image.object_id, "src": image.url },
                "croppedImage": {
                    "id": image.object_id,
                    "src": image.url,
                    "imageWidth": image.width,
                    "imageHeight": image.height,
                    "imageAltText": "",
                },
            }}),
            (None, BannerBackground::Color(color)) => json!({ COLOR_THEME: teams_theme(color) }),
            (None, _) => json!({ COLOR_THEME: teams_theme(DEFAULT_BANNER_COLOR) }),
        };
        json!([{
            "appId": ANNOUNCEMENT_APP_ID,
            "contentType": ANNOUNCEMENT_BANNER_TYPE,
            "content": content,
        }])
        .to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamsBannerImage {
    pub object_id: String,
    pub url: String,
    pub width: u32,
    pub height: u32,
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
    fn teams_cards_carry_the_theme_index_or_the_uploaded_image() {
        let banner = AnnouncementBanner {
            title: "Ship it".into(),
            background: BannerBackground::Color("indigo".into()),
        };
        let cards: Value = serde_json::from_str(&banner.teams_cards(None)).unwrap();
        assert_eq!(cards[0]["appId"], ANNOUNCEMENT_APP_ID);
        assert_eq!(cards[0]["contentType"], ANNOUNCEMENT_BANNER_TYPE);
        assert_eq!(cards[0]["content"], json!({"colorTheme": "7"}));
        let image = TeamsBannerImage {
            object_id: "0-a".into(),
            url: "https://ams.test/0-a".into(),
            width: 1200,
            height: 300,
        };
        let cards: Value = serde_json::from_str(&banner.teams_cards(Some(&image))).unwrap();
        let cropped = &cards[0]["content"]["imageData"]["croppedImage"];
        assert_eq!(cropped["src"], "https://ams.test/0-a");
        assert_eq!(cropped["imageHeight"], 300);
        assert_eq!(cards[0]["content"]["imageData"]["origImage"]["id"], "0-a");
        assert_eq!(teams_theme("unknown"), "0");
    }

    #[test]
    fn local_and_uploaded_images_round_trip() {
        for background in [
            BannerBackground::LocalImage("/home/a/b.png".into()),
            BannerBackground::Image("https://ams.test/0-a".into()),
        ] {
            let banner = AnnouncementBanner {
                title: "Hi".into(),
                background,
            };
            assert_eq!(AnnouncementBanner::parse(&banner.content()), Some(banner));
        }
    }

    #[test]
    fn banner_without_content_is_none() {
        let mut attachment = banner_attachment("");
        attachment.content = None;
        assert_eq!(AnnouncementBanner::from_attachments(&[attachment]), None);
    }
}
