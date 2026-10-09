use gpui_kit::{ImageSource, SharedString};
use teams_core::{Gif, ImageRef};

use crate::stickers::Sticker;

pub const MAX_HEIGHT: u32 = 250;
const STICKER_SIZE: u32 = 250;
const GIF_ITEMTYPE: &str = "http://schema.skype.com/Giphy";
const STICKER_ITEMTYPE: &str = "http://schema.skype.com/Sticker";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteKind {
    Gif,
    Sticker,
}

/// A GIF or sticker the message points at by URL; nothing is uploaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteImage {
    pub kind: RemoteKind,
    pub url: String,
    pub title: String,
    pub width: u32,
    pub height: u32,
}

impl RemoteImage {
    pub fn gif(gif: &Gif) -> Self {
        let (width, height) = capped(gif.width, gif.height);
        RemoteImage {
            kind: RemoteKind::Gif,
            url: gif.url.clone(),
            title: gif.title.clone(),
            width,
            height,
        }
    }

    pub fn sticker(sticker: &Sticker) -> Self {
        RemoteImage {
            kind: RemoteKind::Sticker,
            url: sticker.url(),
            title: sticker.name.to_owned(),
            width: STICKER_SIZE,
            height: STICKER_SIZE,
        }
    }

    pub fn html(&self) -> String {
        let itemtype = match self.kind {
            RemoteKind::Gif => GIF_ITEMTYPE,
            RemoteKind::Sticker => STICKER_ITEMTYPE,
        };
        format!(
            "<img src=\"{}\" width=\"{}\" height=\"{}\" alt=\"{}\" itemtype=\"{itemtype}\">",
            teams_core::escape_html(&self.url),
            self.width,
            self.height,
            teams_core::escape_html(&self.title),
        )
    }

    pub fn image_ref(&self) -> ImageRef {
        ImageRef {
            id: self.url.clone(),
            url: self.url.clone(),
            width: Some(self.width),
            height: Some(self.height),
        }
    }
}

/// Height capped at `MAX_HEIGHT`, aspect ratio kept.
pub fn capped(width: u32, height: u32) -> (u32, u32) {
    if height <= MAX_HEIGHT || height == 0 {
        return (width, height);
    }
    let scaled =
        (u64::from(width) * u64::from(MAX_HEIGHT) + u64::from(height) / 2) / u64::from(height);
    ((scaled as u32).max(1), MAX_HEIGHT)
}

/// Web URLs load through the app's HTTP client, anything else is a local file (demo mode).
pub fn image_source(location: &str) -> ImageSource {
    if location.starts_with("https://") {
        ImageSource::from(SharedString::from(location.to_owned()))
    } else {
        ImageSource::from(std::path::PathBuf::from(location))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gif(width: u32, height: u32) -> Gif {
        Gif {
            url: "https://media0.giphy.com/media/a/giphy.gif".into(),
            preview_url: "https://media0.giphy.com/media/a/200w.gif".into(),
            title: "Say \"hi\" & <wave>".into(),
            width,
            height,
            preview_width: 200,
            preview_height: 200,
        }
    }

    #[test]
    fn tall_gifs_are_capped_keeping_the_aspect_ratio() {
        let image = RemoteImage::gif(&gif(500, 1000));
        assert_eq!((image.width, image.height), (125, 250));
        let small = RemoteImage::gif(&gif(200, 100));
        assert_eq!((small.width, small.height), (200, 100));
    }

    #[test]
    fn gif_html_escapes_the_title_and_names_the_giphy_itemtype() {
        let html = RemoteImage::gif(&gif(358, 360)).html();
        assert_eq!(
            html,
            "<img src=\"https://media0.giphy.com/media/a/giphy.gif\" width=\"249\" height=\"250\" alt=\"Say &quot;hi&quot; &amp; &lt;wave&gt;\" itemtype=\"http://schema.skype.com/Giphy\">"
        );
    }

    #[test]
    fn stickers_are_square_250() {
        let sticker = crate::stickers::popular()[0];
        let html = RemoteImage::sticker(sticker).html();
        assert!(html.contains("width=\"250\" height=\"250\""));
        assert!(html.ends_with("itemtype=\"http://schema.skype.com/Sticker\">"));
        assert!(html.contains("clippy-250x250"));
    }
}
