use unicode_segmentation::UnicodeSegmentation;

use crate::jumbo::is_emoji_grapheme;
use crate::markdown::escape_html;

const VERBATIM_ELEMENTS: [&str; 2] = ["code", "pre"];

pub struct EmoticonTag {
    pub id: String,
    pub title: String,
}

/// Graph turns `<emoji>` into the Teams emoticon markup, so the official client animates it
/// and shows emoji-only messages big. Text inside `code` and `pre` stays as written.
pub fn emoticon_tags(html: &str, lookup: impl Fn(&str) -> Option<EmoticonTag>) -> String {
    let mut output = String::with_capacity(html.len());
    let mut verbatim_depth = 0usize;
    let mut rest = html;
    while !rest.is_empty() {
        let text_end = rest.find('<').unwrap_or(rest.len());
        let (text, after_text) = rest.split_at(text_end);
        if verbatim_depth > 0 {
            output.push_str(text);
        } else {
            push_tagged(&mut output, text, &lookup);
        }
        let tag_end = after_text.find('>').map_or(after_text.len(), |end| end + 1);
        let (tag, after_tag) = after_text.split_at(tag_end);
        verbatim_depth = match tag_name(tag) {
            Some((name, true)) if VERBATIM_ELEMENTS.contains(&name.as_str()) => {
                verbatim_depth.saturating_sub(1)
            }
            Some((name, false)) if VERBATIM_ELEMENTS.contains(&name.as_str()) => verbatim_depth + 1,
            _ => verbatim_depth,
        };
        output.push_str(tag);
        rest = after_tag;
    }
    output
}

fn tag_name(tag: &str) -> Option<(String, bool)> {
    let inner = tag.strip_prefix('<')?;
    let (closing, inner) = match inner.strip_prefix('/') {
        Some(inner) => (true, inner),
        None => (false, inner),
    };
    let name: String = inner
        .chars()
        .take_while(|character| character.is_ascii_alphanumeric())
        .collect();
    Some((name.to_ascii_lowercase(), closing))
}

fn push_tagged(output: &mut String, text: &str, lookup: &impl Fn(&str) -> Option<EmoticonTag>) {
    for grapheme in text.graphemes(true) {
        match is_emoji_grapheme(grapheme)
            .then(|| lookup(grapheme))
            .flatten()
        {
            Some(tag) => output.push_str(&format!(
                "<emoji id=\"{}\" alt=\"{}\" title=\"{}\"></emoji>",
                escape_html(&tag.id),
                grapheme,
                escape_html(&tag.title)
            )),
            None => output.push_str(grapheme),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heart_only(glyph: &str) -> Option<EmoticonTag> {
        glyph.starts_with('\u{2764}').then(|| EmoticonTag {
            id: "heart".into(),
            title: "Heart".into(),
        })
    }

    #[test]
    fn known_emoji_in_text_become_emoji_tags() {
        assert_eq!(
            emoticon_tags("<p>love ❤️ it 😀</p>", heart_only),
            "<p>love <emoji id=\"heart\" alt=\"❤️\" title=\"Heart\"></emoji> it 😀</p>"
        );
    }

    #[test]
    fn code_attributes_and_markup_stay_untouched() {
        let html =
            "<pre><code>❤️</code></pre><p><a href=\"x\" title=\"❤️\">❤️</a> <code>❤️</code></p>";
        assert_eq!(
            emoticon_tags(html, heart_only),
            "<pre><code>❤️</code></pre><p><a href=\"x\" title=\"❤️\"><emoji id=\"heart\" alt=\"❤️\" title=\"Heart\"></emoji></a> <code>❤️</code></p>"
        );
    }
}
