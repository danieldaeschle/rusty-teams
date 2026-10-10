use chatsvc::{Language, language_code};

const MIN_CONFIDENCE: u32 = 70;
const LATIN_MIN_CHARS: usize = 30;
const CYRILLIC_MIN_CHARS: usize = 35;
const OTHER_SCRIPT_MIN_CHARS: usize = 15;
const CHINESE_MIN_CHARS: usize = 10;
pub const FALLBACK_TARGET: &str = "en";

const CYRILLIC: [&str; 11] = [
    "ru", "uk", "bg", "be", "mk", "sr", "kk", "ky", "mn", "tg", "tt",
];

const OTHER_SCRIPTS: [&str; 30] = [
    "ar", "fa", "ur", "ps", "sd", "ug", "he", "yi", "ja", "ko", "th", "hi", "bn", "ta", "te", "kn",
    "ml", "mr", "gu", "pa", "ne", "si", "el", "hy", "ka", "am", "km", "lo", "my", "dv",
];

const NAMES: [(&str, &str); 30] = [
    ("ar", "Arabic"),
    ("bg", "Bulgarian"),
    ("cs", "Czech"),
    ("da", "Danish"),
    ("de", "German"),
    ("el", "Greek"),
    ("en", "English"),
    ("es", "Spanish"),
    ("fi", "Finnish"),
    ("fr", "French"),
    ("he", "Hebrew"),
    ("hi", "Hindi"),
    ("hu", "Hungarian"),
    ("it", "Italian"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("nl", "Dutch"),
    ("no", "Norwegian"),
    ("pl", "Polish"),
    ("pt", "Portuguese"),
    ("ro", "Romanian"),
    ("ru", "Russian"),
    ("sk", "Slovak"),
    ("sv", "Swedish"),
    ("th", "Thai"),
    ("tr", "Turkish"),
    ("uk", "Ukrainian"),
    ("vi", "Vietnamese"),
    ("zh-chs", "Chinese (Simplified)"),
    ("zh-cht", "Chinese (Traditional)"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageStamp {
    pub candidates: Vec<(String, u32)>,
    pub length: Option<usize>,
}

impl LanguageStamp {
    /// Reads `languages=de:100;en:79;length:82;&detector=Bling`.
    pub fn parse(text: &str) -> Option<LanguageStamp> {
        let mut candidates = Vec::new();
        let mut length = None;
        for segment in text.split(';') {
            let segment = segment.trim().trim_start_matches("languages=");
            let Some((key, value)) = segment.split_once(':') else {
                continue;
            };
            if key == "length" {
                length = value.parse().ok();
            } else if let Ok(score) = value.parse::<u32>() {
                candidates.push((language_code(key), score));
            }
        }
        (!candidates.is_empty()).then_some(LanguageStamp { candidates, length })
    }

    fn score_of(&self, code: &str) -> Option<u32> {
        self.candidates
            .iter()
            .find(|(candidate, _)| candidate == code)
            .map(|(_, score)| *score)
    }
}

pub fn min_chars(code: &str) -> usize {
    if code.starts_with("zh") {
        CHINESE_MIN_CHARS
    } else if CYRILLIC.contains(&code) {
        CYRILLIC_MIN_CHARS
    } else if OTHER_SCRIPTS.contains(&code) {
        OTHER_SCRIPT_MIN_CHARS
    } else {
        LATIN_MIN_CHARS
    }
}

/// `None` when the detector is not confident or the message reads as the target language.
pub fn confident_language(stamp: &LanguageStamp, target: &str) -> Option<String> {
    let target = language_code(target);
    let (top, _) = stamp.candidates.first()?;
    let length = stamp.length?;
    if *top != target && length < min_chars(top) {
        return None;
    }
    if stamp
        .score_of(&target)
        .is_some_and(|score| score >= MIN_CONFIDENCE)
    {
        return None;
    }
    Some(top.clone())
}

/// The language to name in the offer, or `None` when no offer is due.
pub fn offered_language(stamp: &str, target: &str, never: &[String]) -> Option<String> {
    let source = confident_language(&LanguageStamp::parse(stamp)?, target)?;
    let target = language_code(target);
    let known = source == target || never.iter().any(|code| language_code(code) == source);
    (!known).then_some(source)
}

pub fn default_target(
    target_locale: Option<&str>,
    display_locale: Option<&str>,
    supported: &[Language],
) -> String {
    if let Some(locale) = target_locale {
        return language_code(locale);
    }
    let display = display_locale.map(language_code);
    match display {
        Some(code)
            if supported.is_empty() || supported.iter().any(|language| language.code == code) =>
        {
            code
        }
        _ => FALLBACK_TARGET.to_owned(),
    }
}

pub fn language_name(code: &str, languages: &[Language]) -> String {
    let code = language_code(code);
    if let Some(language) = languages
        .iter()
        .find(|language| language_code(&language.code) == code)
    {
        return language.name.clone();
    }
    NAMES
        .iter()
        .find(|(known, _)| *known == code)
        .map_or_else(|| code.to_uppercase(), |(_, name)| (*name).to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer(stamp: &str, target: &str, never: &[&str]) -> Option<String> {
        let never: Vec<String> = never.iter().map(|code| (*code).to_owned()).collect();
        offered_language(stamp, target, &never)
    }

    #[test]
    fn parses_candidates_and_length() {
        let stamp =
            LanguageStamp::parse("languages=de:100;en:79;nl:70;length:82;&detector=Bling").unwrap();
        assert_eq!(
            stamp.candidates,
            vec![("de".into(), 100), ("en".into(), 79), ("nl".into(), 70)]
        );
        assert_eq!(stamp.length, Some(82));
        assert_eq!(LanguageStamp::parse("garbage"), None);
        assert_eq!(LanguageStamp::parse(""), None);
    }

    #[test]
    fn a_confident_foreign_message_is_offered() {
        assert_eq!(
            offer(
                "languages=fr:100;en:20;length:60;&detector=Bling",
                "en",
                &[]
            )
            .as_deref(),
            Some("fr")
        );
    }

    #[test]
    fn no_length_is_low_confidence() {
        assert_eq!(offer("languages=fr:100;en:20", "en", &[]), None);
    }

    #[test]
    fn short_latin_text_is_low_confidence() {
        assert_eq!(offer("languages=fr:100;length:29", "en", &[]), None);
        assert_eq!(
            offer("languages=fr:100;length:30", "en", &[]).as_deref(),
            Some("fr")
        );
    }

    #[test]
    fn thresholds_follow_the_script_of_the_top_language() {
        assert_eq!(offer("languages=ru:100;length:34", "en", &[]), None);
        assert_eq!(
            offer("languages=ru:100;length:35", "en", &[]).as_deref(),
            Some("ru")
        );
        assert_eq!(offer("languages=ar:100;length:14", "en", &[]), None);
        assert_eq!(
            offer("languages=ar:100;length:15", "en", &[]).as_deref(),
            Some("ar")
        );
        assert_eq!(
            offer("languages=ja:100;length:15", "en", &[]).as_deref(),
            Some("ja")
        );
        assert_eq!(offer("languages=zh-chs:100;length:9", "en", &[]), None);
        assert_eq!(
            offer("languages=zh-chs:100;length:10", "en", &[]).as_deref(),
            Some("zh-chs")
        );
    }

    #[test]
    fn a_strong_target_score_reads_as_the_target_language() {
        assert_eq!(offer("languages=de:100;en:70;length:80", "en", &[]), None);
        assert_eq!(
            offer("languages=de:100;en:69;length:80", "en", &[]).as_deref(),
            Some("de")
        );
    }

    #[test]
    fn the_target_language_itself_is_never_offered() {
        assert_eq!(offer("languages=en:100;length:80", "en", &[]), None);
        assert_eq!(offer("languages=en:50;length:80", "en-GB", &[]), None);
        assert_eq!(offer("languages=en:50;length:5", "en", &[]), None);
    }

    #[test]
    fn languages_on_the_never_list_are_not_offered() {
        let stamp = "languages=de:100;length:80";
        assert_eq!(offer(stamp, "en", &["de"]), None);
        assert_eq!(offer(stamp, "en", &["de-DE"]), None);
        assert_eq!(offer(stamp, "en", &["fr"]).as_deref(), Some("de"));
    }

    #[test]
    fn target_defaults_to_the_display_locale_then_english() {
        let supported = vec![Language {
            code: "de".into(),
            name: "German".into(),
        }];
        assert_eq!(
            default_target(Some("fr-FR"), Some("de-DE"), &supported),
            "fr"
        );
        assert_eq!(default_target(None, Some("de-DE"), &supported), "de");
        assert_eq!(default_target(None, Some("xx-XX"), &supported), "en");
        assert_eq!(default_target(None, None, &supported), "en");
        assert_eq!(default_target(None, Some("en-US"), &[]), "en");
    }

    #[test]
    fn names_come_from_the_list_then_the_built_in_table() {
        let listed = vec![Language {
            code: "de".into(),
            name: "Deutsch".into(),
        }];
        assert_eq!(language_name("de", &listed), "Deutsch");
        assert_eq!(language_name("fr", &listed), "French");
        assert_eq!(language_name("zh-TW", &[]), "Chinese (Traditional)");
        assert_eq!(language_name("qq", &[]), "QQ");
    }
}
