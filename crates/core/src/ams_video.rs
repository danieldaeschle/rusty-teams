//! Teams video clips: an AMS `<video src=".../views/video">`, shown through the object's thumbnail view.

const VIDEO_VIEW: &str = "/views/video";
const THUMBNAIL_VIEW: &str = "/views/thumbnail";

/// The thumbnail view of the AMS video clip at `src`.
pub fn video_thumbnail_url(src: &str) -> Option<String> {
    let object = src.strip_suffix(VIDEO_VIEW)?;
    chatsvc::is_ams_image_url(src).then(|| format!("{object}{THUMBNAIL_VIEW}"))
}

/// The clip behind a thumbnail from [`video_thumbnail_url`].
pub fn thumbnail_video_url(url: &str) -> Option<String> {
    let object = url.strip_suffix(THUMBNAIL_VIEW)?;
    chatsvc::is_ams_image_url(url).then(|| format!("{object}{VIDEO_VIEW}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIP: &str = "https://eu-api.asm.skype.com/v1/objects/0-weu-d19-abc/views/video";

    #[test]
    fn an_ams_clip_is_shown_by_its_thumbnail_and_played_from_it() {
        let thumbnail = video_thumbnail_url(CLIP).unwrap();
        assert_eq!(
            thumbnail,
            "https://eu-api.asm.skype.com/v1/objects/0-weu-d19-abc/views/thumbnail"
        );
        assert_eq!(thumbnail_video_url(&thumbnail).as_deref(), Some(CLIP));
        assert_eq!(
            video_thumbnail_url("https://x.example/v1/objects/a/views/video"),
            None
        );
        assert_eq!(
            thumbnail_video_url("https://eu-api.asm.skype.com/v1/objects/a/views/imgo"),
            None
        );
    }
}
