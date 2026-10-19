use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::OnceLock;

const EMOTIONS: &str = include_str!("../assets/emotions.tsv");
const EMOTICONS: &str = include_str!("../../app/assets/emoticons.tsv");
const SKIN_TONES: std::ops::RangeInclusive<char> = '\u{1f3fb}'..='\u{1f3ff}';
const TONE_MARKER: &str = "-tone";
const ZERO_WIDTH_JOINER: char = '\u{200d}';
const VARIATION_SELECTOR: char = '\u{fe0f}';
const MAX_TONE: u8 = 5;
const CUSTOM_EMOJI_SEPARATOR: char = ';';
const AMS_OBJECTS_BASE: &str = "https://de-prod.asyncgw.teams.microsoft.com/v1/objects/";
const CUSTOM_EMOJI_VIEW: &str = "/views/imgt2_anim";

type Index = HashMap<&'static str, Vec<&'static str>>;

fn index() -> &'static Index {
    static PARSED: OnceLock<Index> = OnceLock::new();
    PARSED.get_or_init(|| {
        EMOTIONS
            .lines()
            .filter_map(|line| {
                let (glyph, keys) = line.split_once('\t')?;
                Some((glyph, keys.split(' ').collect()))
            })
            .collect()
    })
}

fn diverse_ids() -> &'static std::collections::HashSet<&'static str> {
    static PARSED: OnceLock<std::collections::HashSet<&'static str>> = OnceLock::new();
    PARSED.get_or_init(|| {
        EMOTICONS
            .lines()
            .filter_map(|line| {
                let fields: Vec<&str> = line.split('\t').collect();
                (fields.get(4) == Some(&"1")).then_some(fields[0])
            })
            .collect()
    })
}

fn split_tone(glyph: &str) -> (String, Option<u32>) {
    let tone = glyph
        .chars()
        .find(|character| SKIN_TONES.contains(character))
        .map(|tone| tone as u32 - *SKIN_TONES.start() as u32 + 1);
    let base = glyph
        .chars()
        .filter(|character| !SKIN_TONES.contains(character))
        .collect();
    (base, tone)
}

/// Teams sends a toned reaction as `{diverse id}-tone{n}`, so 👍🏼 is `yes-tone2`, not `like`.
pub fn reaction_key(glyph: &str) -> Option<String> {
    let (base, tone) = split_tone(glyph);
    let keys = emotion_keys(&base);
    match tone {
        Some(tone) => keys
            .iter()
            .find(|key| diverse_ids().contains(*key))
            .map(|key| format!("{key}{TONE_MARKER}{tone}"))
            .or_else(|| keys.first().map(|key| key.to_string())),
        None => keys.first().map(|key| key.to_string()),
    }
}

/// Graph reads a toned reaction back without its tone, so removing one also clears the plain keys.
pub fn reaction_keys(glyph: &str) -> Vec<String> {
    let (base, tone) = split_tone(glyph);
    let plain = emotion_keys(&base).iter().map(|key| key.to_string());
    match tone {
        Some(_) => {
            let toned = reaction_key(glyph);
            toned
                .clone()
                .into_iter()
                .chain(plain.filter(|key| Some(key) != toned.as_ref()))
                .collect()
        }
        None => plain.collect(),
    }
}

/// The Teams rule: the modifier goes in once, before the first joiner, otherwise at the end;
/// a variation selector at that spot is dropped.
pub fn insert_skin_tone(base: &str, tone: u8) -> String {
    let Some(modifier) = (1..=MAX_TONE)
        .contains(&tone)
        .then(|| char::from_u32(*SKIN_TONES.start() as u32 + u32::from(tone) - 1))
        .flatten()
    else {
        return base.to_owned();
    };
    let (head, tail) = base.split_at(base.find(ZERO_WIDTH_JOINER).unwrap_or(base.len()));
    format!(
        "{}{modifier}{tail}",
        head.strip_suffix(VARIATION_SELECTOR).unwrap_or(head)
    )
}

/// `yes-tone2` is 👍🏼: the chat service key of a reaction as the glyph Graph would report,
/// but with the tone Graph drops.
pub fn reaction_glyph_for_key(key: &str) -> Option<String> {
    let (base_key, tone) = match key.rsplit_once(TONE_MARKER) {
        Some((base, tone)) => match tone.parse::<u8>() {
            Ok(tone) => (base, Some(tone)),
            Err(_) => (key, None),
        },
        None => (key, None),
    };
    let base = emotion_glyph(base_key)?;
    Some(match tone {
        Some(tone) => insert_skin_tone(base, tone),
        None => base.to_owned(),
    })
}

pub fn emotion_glyph(key: &str) -> Option<&'static str> {
    static GLYPHS: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    GLYPHS
        .get_or_init(|| {
            EMOTIONS
                .lines()
                .filter_map(|line| line.split_once('\t'))
                .flat_map(|(glyph, keys)| keys.split(' ').map(move |key| (key, glyph)))
                .collect()
        })
        .get(key)
        .copied()
}

pub fn emotion_keys(glyph: &str) -> &'static [&'static str] {
    let bare = if glyph.contains('\u{FE0F}') {
        Cow::Owned(glyph.replace('\u{FE0F}', ""))
    } else {
        Cow::Borrowed(glyph)
    };
    index().get(bare.as_ref()).map_or(&[], Vec::as_slice)
}

pub fn emotion_key(glyph: &str) -> Option<&'static str> {
    emotion_keys(glyph).first().copied()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CustomEmoji<'a> {
    pub name: &'a str,
    pub object_id: &'a str,
}

impl CustomEmoji<'_> {
    pub fn image_url(&self) -> String {
        format!("{AMS_OBJECTS_BASE}{}{CUSTOM_EMOJI_VIEW}", self.object_id)
    }
}

/// `absolute-cinema;0-wcde-d3-5f5e...`: an org emoji, name and AMS object of its image.
pub fn custom_emoji(key: &str) -> Option<CustomEmoji<'_>> {
    let (name, object_id) = key.split_once(CUSTOM_EMOJI_SEPARATOR)?;
    let valid_object = !object_id.is_empty()
        && object_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-');
    (!name.is_empty() && valid_object).then_some(CustomEmoji { name, object_id })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_emoji_key_maps_to_its_ams_image() {
        let emoji = custom_emoji("absolute-cinema;0-wcde-d3-5f5e4f463d6bb959b08614fd1d9680b8")
            .expect("custom emoji");
        assert_eq!(emoji.name, "absolute-cinema");
        assert_eq!(
            emoji.image_url(),
            "https://de-prod.asyncgw.teams.microsoft.com/v1/objects/0-wcde-d3-5f5e4f463d6bb959b08614fd1d9680b8/views/imgt2_anim"
        );
        assert_eq!(custom_emoji("like"), None);
        assert_eq!(custom_emoji("x;../evil"), None);
        assert_eq!(custom_emoji(";0-wcde-d3-1"), None);
    }

    #[test]
    fn resolves_teams_keys() {
        assert_eq!(
            emotion_key("\u{1F603}"),
            Some("1f603_grinningfacewithbigeyes")
        );
        assert_eq!(emotion_key("\u{1F44D}"), Some("like"));
    }

    #[test]
    fn ignores_variation_selectors() {
        assert_eq!(emotion_key("\u{2764}"), Some("heart"));
        assert_eq!(emotion_key("\u{2764}\u{FE0F}"), Some("heart"));
    }

    #[test]
    fn lists_alias_keys_with_the_primary_first() {
        assert_eq!(
            emotion_keys("\u{1F603}"),
            ["1f603_grinningfacewithbigeyes", "happyface"]
        );
        assert_eq!(emotion_keys("\u{2764}\u{FE0F}")[0], "heart");
    }

    #[test]
    fn toned_reactions_use_the_diverse_id_with_the_tone() {
        assert_eq!(reaction_key("👍").as_deref(), Some("like"));
        assert_eq!(reaction_key("👍🏼").as_deref(), Some("yes-tone2"));
        assert_eq!(reaction_keys("👍🏿"), ["yes-tone5", "like", "yes"]);
        assert_eq!(reaction_keys("👍"), ["like", "yes"]);
        assert_eq!(reaction_key("❤️").as_deref(), Some("heart"));
    }

    #[test]
    fn keys_turn_into_glyphs_with_their_tone() {
        assert_eq!(reaction_glyph_for_key("yes-tone2").as_deref(), Some("👍🏼"));
        assert_eq!(reaction_glyph_for_key("like").as_deref(), Some("👍"));
        assert_eq!(reaction_glyph_for_key("heart").as_deref(), Some("❤"));
        assert_eq!(reaction_glyph_for_key("nope"), None);
        assert_eq!(insert_skin_tone("🤷\u{200d}♂️", 1), "🤷🏻\u{200d}♂️");
        assert_eq!(insert_skin_tone("✌️", 3), "✌🏽");
    }

    #[test]
    fn unknown_glyph_has_no_key() {
        assert_eq!(emotion_key("x"), None);
        assert!(emotion_keys("x").is_empty());
    }
}
