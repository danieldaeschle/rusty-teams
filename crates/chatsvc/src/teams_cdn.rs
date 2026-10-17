pub const HOST: &str = "statics.teams.cdn.office.net";
pub const ROOT: &str = "https://statics.teams.cdn.office.net";
pub const SOUND_BASE_URL: &str = "https://statics.teams.cdn.office.net/evergreen-assets/audio/";
pub const STICKER_BASE_URL: &str =
    "https://statics.teams.cdn.office.net/evergreen-assets/stickerassets";
pub const EMOTICON_BASE_URL: &str = "https://statics.teams.cdn.office.net/evergreen-assets/personal-expressions/v2/assets/emoticons";
pub const BACKGROUND_CATALOG_URL: &str =
    "https://statics.teams.cdn.office.net/evergreen-assets/backgroundimages/config.json?v=7";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_url_lives_on_the_cdn_root() {
        assert_eq!(ROOT, format!("https://{HOST}"));
        for url in [
            SOUND_BASE_URL,
            STICKER_BASE_URL,
            EMOTICON_BASE_URL,
            BACKGROUND_CATALOG_URL,
        ] {
            assert!(
                url.starts_with(&format!("{ROOT}/evergreen-assets/")),
                "{url}"
            );
        }
    }
}
