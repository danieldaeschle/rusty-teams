mod strip;

use std::collections::HashSet;
use std::sync::OnceLock;
use std::time::Duration;

use teams_core::EmoticonTag;

pub use strip::{StripImages, StripLoader, StripSource, split_strip};

const CATALOG: &str = include_str!("../../../app/assets/emoticons.tsv");
const ASSET_BASE: &str = "https://statics.teams.cdn.office.net/evergreen-assets/personal-expressions/v2/assets/emoticons";
const ASSET_SIZE: u32 = 100;
const FRAMES_PER_SECOND: u64 = 24;
const VARIATION_SELECTOR: char = '\u{fe0f}';
const SKIN_TONES: std::ops::RangeInclusive<char> = '\u{1f3fb}'..='\u{1f3ff}';
const TONE_NAMES: [&str; 5] = ["light", "medium light", "medium", "medium dark", "dark"];
const TONE_ID_MARKER: &str = "-tone";

#[derive(Debug, PartialEq, Eq)]
pub struct Emoticon {
    pub id: &'static str,
    pub unicode: &'static str,
    etag: &'static str,
    frames: u32,
    diverse: bool,
    description: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedEmoticon {
    pub emoticon: &'static Emoticon,
    tone: Option<u32>,
}

impl ResolvedEmoticon {
    pub fn strip_source(&self) -> StripSource {
        let tone = match self.tone {
            Some(tone) if self.emoticon.diverse => format!("_s{}", tone + 1),
            _ => String::new(),
        };
        StripSource {
            url: format!(
                "{ASSET_BASE}/{}/default/{ASSET_SIZE}_anim_f{tone}.png?v={}",
                self.emoticon.id, self.emoticon.etag
            )
            .into(),
            frame_duration: Duration::from_millis(1000 / FRAMES_PER_SECOND),
        }
    }

    pub fn loop_duration(&self) -> Duration {
        Duration::from_millis(u64::from(self.emoticon.frames) * 1000 / FRAMES_PER_SECOND)
    }
}

fn catalog() -> &'static [Emoticon] {
    static PARSED: OnceLock<Vec<Emoticon>> = OnceLock::new();
    PARSED.get_or_init(|| {
        CATALOG
            .lines()
            .filter_map(|line| {
                let mut fields = line.split('\t');
                Some(Emoticon {
                    id: fields.next()?,
                    unicode: fields.next()?,
                    etag: fields.next()?,
                    frames: fields.next()?.parse().ok()?,
                    diverse: fields.next()? == "1",
                    description: fields.next()?,
                })
            })
            .collect()
    })
}

fn skin_tone(glyph: &str) -> Option<u32> {
    glyph
        .chars()
        .find(|character| SKIN_TONES.contains(character))
        .map(|tone| tone as u32 - *SKIN_TONES.start() as u32 + 1)
}

pub const SKIN_TONE_COUNT: u8 = 5;

fn toneable_glyphs() -> &'static HashSet<&'static str> {
    static PARSED: OnceLock<HashSet<&'static str>> = OnceLock::new();
    PARSED.get_or_init(|| {
        catalog()
            .iter()
            .filter(|emoticon| emoticon.diverse)
            .map(|emoticon| emoticon.unicode)
            .collect()
    })
}

pub fn is_toneable(glyph: &str) -> bool {
    toneable_glyphs().contains(base_glyph(glyph).as_str())
}

pub fn without_tone(glyph: &str) -> String {
    glyph
        .chars()
        .filter(|character| !SKIN_TONES.contains(character))
        .collect()
}

pub fn with_tone(glyph: &str, tone: u8) -> String {
    let base = without_tone(glyph);
    if tone == 0 || tone > SKIN_TONE_COUNT || !is_toneable(&base) {
        return base;
    }
    chatsvc::insert_skin_tone(&base, tone)
}

fn base_glyph(glyph: &str) -> String {
    glyph
        .chars()
        .filter(|character| *character != VARIATION_SELECTOR && !SKIN_TONES.contains(character))
        .collect()
}

pub fn resolve(id: Option<&str>, glyph: &str) -> Option<ResolvedEmoticon> {
    let base = base_glyph(glyph);
    let id = id.map(|id| id.split_once(TONE_ID_MARKER).map_or(id, |(base, _)| base));
    let by_id = id.and_then(|id| catalog().iter().find(|emoticon| emoticon.id == id));
    let by_unicode = || {
        let mut matches = catalog().iter().filter(|emoticon| emoticon.unicode == base);
        let first = matches.next()?;
        Some(
            std::iter::once(first)
                .chain(matches)
                .find(|emoticon| emoticon.diverse)
                .unwrap_or(first),
        )
    };
    Some(ResolvedEmoticon {
        emoticon: by_id.or_else(by_unicode)?,
        tone: skin_tone(glyph),
    })
}

pub fn send_tag(glyph: &str) -> Option<EmoticonTag> {
    let resolved = resolve(None, glyph)?;
    let emoticon = resolved.emoticon;
    let (id, title, suffix) = match resolved.tone.filter(|_| emoticon.diverse) {
        Some(tone) => (
            format!("{}{TONE_ID_MARKER}{tone}", emoticon.id),
            format!(
                "{} with {} skin tone",
                emoticon.description,
                TONE_NAMES[tone as usize - 1]
            ),
            format!("_s{}", tone + 1),
        ),
        None => (
            emoticon.id.to_owned(),
            emoticon.description.to_owned(),
            String::new(),
        ),
    };
    Some(EmoticonTag {
        id,
        title,
        image_url: format!(
            "{ASSET_BASE}/{}/default/{{size}}_f{suffix}.png",
            emoticon.id
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_message_id_picks_the_emoticon_over_the_first_unicode_match() {
        assert_eq!(
            resolve(Some("laughdog"), "😄").unwrap().emoticon.id,
            "laughdog"
        );
        assert_eq!(
            resolve(None, "😄").unwrap().emoticon.id,
            "grinningfacewithsmilingeyes"
        );
        assert_eq!(resolve(Some("unknown"), "❤️").unwrap().emoticon.id, "heart");
        assert_eq!(resolve(None, "👍").unwrap().emoticon.id, "yes");
        assert!(resolve(None, "x").is_none());
    }

    #[test]
    fn send_tags_carry_the_tone_id_and_toned_image() {
        let heart = send_tag("❤️").unwrap();
        assert_eq!(
            (heart.id.as_str(), heart.title.as_str()),
            ("heart", "Heart")
        );
        assert!(heart.image_url.ends_with("/heart/default/{size}_f.png"));
        let thumb = send_tag("👍🏼").unwrap();
        assert_eq!(thumb.id, "yes-tone2");
        assert_eq!(thumb.title, "Yes with medium light skin tone");
        assert!(thumb.image_url.ends_with("/yes/default/{size}_f_s3.png"));
        assert_eq!(send_tag("👍").unwrap().id, "yes");
        assert_eq!(resolve(Some("yes-tone2"), "👍🏼").unwrap().emoticon.id, "yes");
    }

    #[test]
    fn tones_follow_the_teams_insertion_rule() {
        assert!(is_toneable("👍"));
        assert!(!is_toneable("❤️"));
        assert_eq!(with_tone("👍", 2), "👍🏼");
        assert_eq!(with_tone("👍🏿", 0), "👍");
        assert_eq!(with_tone("✌️", 3), "✌🏽");
        assert_eq!(with_tone("🤷\u{200d}♂️", 1), "🤷🏻\u{200d}♂️");
        assert_eq!(with_tone("❤️", 2), "❤️");
    }

    #[test]
    fn urls_use_the_100_strip_with_etag_and_tone_only_for_diverse_emoticons() {
        let heart = resolve(None, "❤️").unwrap();
        assert!(
            heart
                .strip_source()
                .url
                .ends_with("/heart/default/100_anim_f.png?v=v34")
        );
        assert_eq!(heart.loop_duration(), Duration::from_millis(73 * 1000 / 24));
        let thumb = resolve(None, "👍🏽").unwrap();
        assert!(
            thumb
                .strip_source()
                .url
                .contains("/yes/default/100_anim_f_s4.png?v=")
        );
    }
}
