use std::ops::Range;

const MIN_QUERY_CHARS: usize = 2;
const MAX_CODE_CHARS: usize = 40;
const SMILEYS: [(&str, &str); 6] = [
    (":-)", "🙂"),
    (":)", "🙂"),
    (":(", "🙁"),
    (":D", "😀"),
    (";)", "😉"),
    ("<3", "❤️"),
];

fn opens_word(before: &str) -> bool {
    before
        .chars()
        .next_back()
        .is_none_or(|previous| previous.is_whitespace() || previous == '(')
}

fn is_code_char(character: char) -> bool {
    character.is_alphanumeric() || matches!(character, '_' | '-' | '+')
}

fn is_code(code: &str) -> bool {
    !code.is_empty() && code.chars().count() <= MAX_CODE_CHARS && code.chars().all(is_code_char)
}

/// The one character typed just before `cursor`, if that is the only difference to `previous`.
pub fn typed_char(previous: &str, value: &str, cursor: usize) -> Option<char> {
    let typed = value.get(..cursor)?.chars().next_back()?;
    let start = cursor - typed.len_utf8();
    let one_more = value.len() == previous.len() + typed.len_utf8()
        && previous.get(..start)? == &value[..start]
        && previous.get(start..)? == &value[cursor..];
    one_more.then_some(typed)
}

/// `:th|` -> the open query `th`. `10:30`, `Hinweis: `, `:+1` and `a:b` never open.
pub fn active_query(text: &str, cursor: usize) -> Option<(Range<usize>, String)> {
    let before = text.get(..cursor)?;
    let colon = before.rfind(':')?;
    let query = &before[colon + 1..];
    let first = query.chars().next()?;
    let opens = opens_word(&before[..colon])
        && (first.is_alphabetic() || matches!(first, '+' | '-'))
        && query.chars().count() >= MIN_QUERY_CHARS
        && is_code(query)
        && query.chars().any(char::is_alphabetic);
    opens.then(|| (colon..cursor, query.to_owned()))
}

/// `:thumbsup:|` -> range of the whole code and the code itself.
pub fn closing_code(text: &str, cursor: usize) -> Option<(Range<usize>, &str)> {
    let before = text.get(..cursor)?.strip_suffix(':')?;
    let colon = before.rfind(':')?;
    let code = &before[colon + 1..];
    (opens_word(&before[..colon]) && is_code(code)).then_some((colon..cursor, code))
}

fn smiley_ending_at(text: &str, end: usize) -> Option<(Range<usize>, &'static str)> {
    let before = text.get(..end)?;
    SMILEYS.iter().find_map(|(smiley, glyph)| {
        let start = end.checked_sub(smiley.len())?;
        (before.ends_with(smiley) && opens_word(&before[..start])).then_some((start..end, *glyph))
    })
}

/// `:) |` -> range of `:)` and its emoji.
pub fn smiley_before_space(text: &str, cursor: usize) -> Option<(Range<usize>, &'static str)> {
    let end = cursor.checked_sub(1)?;
    (text.get(end..cursor)? == " ")
        .then(|| smiley_ending_at(text, end))
        .flatten()
}

/// The smiley that ends the text, converted on Enter.
pub fn trailing_smiley(text: &str) -> Option<(Range<usize>, &'static str)> {
    smiley_ending_at(text, text.trim_end().len())
}

#[cfg(test)]
mod tests {
    use super::{active_query, closing_code, smiley_before_space, trailing_smiley, typed_char};

    fn query(text: &str) -> Option<String> {
        active_query(text, text.len()).map(|(_, query)| query)
    }

    #[test]
    fn typed_char_sees_only_single_insertions() {
        assert_eq!(typed_char("a:ok", "a:ok:", 5), Some(':'));
        assert_eq!(typed_char("ab", "a:b", 2), Some(':'));
        assert_eq!(typed_char("👍", ":+1:", 4), None);
        assert_eq!(typed_char("a", "a::", 3), None);
    }

    #[test]
    fn colon_at_word_start_opens_after_two_letters() {
        assert_eq!(active_query("Danke :th", 9), Some((6..9, "th".into())));
        assert_eq!(query(":dau"), Some("dau".into()));
        assert_eq!(query("(:herz"), Some("herz".into()));
        assert_eq!(query("Danke :t"), None);
    }

    #[test]
    fn umlauts_count_as_letters() {
        assert_eq!(query(":drü"), Some("drü".into()));
    }

    #[test]
    fn times_labels_urls_and_numbers_stay_text() {
        assert_eq!(query("Treffen um 10:30"), None);
        assert_eq!(query("Hinweis: bitte"), None);
        assert_eq!(query("Siehe https://example"), None);
        assert_eq!(query("Ergebnis:ok"), None);
        assert_eq!(query("Faktor 3:2"), None);
        assert_eq!(query("Okay :+1"), None);
        assert_eq!(query(":th\nx"), None);
    }

    #[test]
    fn closing_colon_yields_the_code() {
        let text = "Gute Arbeit :thumbsup:";
        assert_eq!(closing_code(text, text.len()), Some((12..22, "thumbsup")));
        assert_eq!(closing_code("Okay :+1:", 9), Some((5..9, "+1")));
        assert_eq!(closing_code("um 10:30:", 9), None);
        assert_eq!(closing_code("Hinweis::", 9), None);
    }

    #[test]
    fn smiley_converts_on_space_at_word_start() {
        assert_eq!(smiley_before_space("Hi :) ", 6), Some((3..5, "🙂")));
        assert_eq!(smiley_before_space("Hi :-) ", 7), Some((3..6, "🙂")));
        assert_eq!(smiley_before_space("<3 ", 3), Some((0..2, "❤️")));
        assert_eq!(smiley_before_space("(Hinweis:) ", 11), None);
        assert_eq!(smiley_before_space("a:D ", 4), None);
        assert_eq!(smiley_before_space("Hi :)", 5), None);
    }

    #[test]
    fn trailing_smiley_converts_on_enter() {
        assert_eq!(trailing_smiley("Danke ;)"), Some((6..8, "😉")));
        assert_eq!(trailing_smiley("Danke ;) "), Some((6..8, "😉")));
        assert_eq!(trailing_smiley("a:D/b"), None);
    }
}
