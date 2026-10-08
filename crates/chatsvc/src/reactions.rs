use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::OnceLock;

const EMOTIONS: &str = include_str!("../assets/emotions.tsv");

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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn unknown_glyph_has_no_key() {
        assert_eq!(emotion_key("x"), None);
        assert!(emotion_keys("x").is_empty());
    }
}
