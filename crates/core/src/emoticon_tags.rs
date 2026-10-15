use unicode_segmentation::UnicodeSegmentation;

use crate::jumbo::{is_emoji_grapheme, jumbo_emoji};
use crate::markdown::escape_html;

const VERBATIM_ELEMENTS: [&str; 2] = ["code", "pre"];
const INLINE_SIZE: u32 = 20;
const JUMBO_SIZE: u32 = 50;
const SIZE_PLACEHOLDER: &str = "{size}";

pub struct EmoticonTag {
    pub id: String,
    pub title: String,
    pub image_url: String,
}

/// The Teams composer markup. Graph keeps it; a Graph `<emoji>` tag would be re-resolved from
/// the unicode alone, which loses skin tones and picks the wrong emoticon for some characters.
pub fn emoticon_tags(html: &str, lookup: impl Fn(&str) -> Option<EmoticonTag>) -> String {
    let inline = tagged(html, &lookup, INLINE_SIZE);
    if inline != html && jumbo_emoji(&inline).is_some() {
        tagged(html, &lookup, JUMBO_SIZE)
    } else {
        inline
    }
}

fn tagged(html: &str, lookup: &impl Fn(&str) -> Option<EmoticonTag>, size: u32) -> String {
    let mut output = String::with_capacity(html.len());
    let mut verbatim_depth = 0usize;
    let mut rest = html;
    while !rest.is_empty() {
        let text_end = rest.find('<').unwrap_or(rest.len());
        let (text, after_text) = rest.split_at(text_end);
        if verbatim_depth > 0 {
            output.push_str(text);
        } else {
            push_tagged(&mut output, text, lookup, size);
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

fn push_tagged(
    output: &mut String,
    text: &str,
    lookup: &impl Fn(&str) -> Option<EmoticonTag>,
    size: u32,
) {
    for grapheme in text.graphemes(true) {
        match is_emoji_grapheme(grapheme)
            .then(|| lookup(grapheme))
            .flatten()
        {
            Some(tag) => output.push_str(&span(&tag, grapheme, size)),
            None => output.push_str(grapheme),
        }
    }
}

fn span(tag: &EmoticonTag, glyph: &str, size: u32) -> String {
    let id = escape_html(&tag.id);
    let title = escape_html(&tag.title);
    let source = escape_html(&tag.image_url.replace(SIZE_PLACEHOLDER, &size.to_string()));
    format!(
        "<span contenteditable=\"false\" title=\"{title}\" type=\"({id})\" class=\"animated-emoticon-{size}-{id}\" itemscope=\"\"><img itemscope=\"\" itemtype=\"http://schema.skype.com/Emoji\" itemid=\"{id}\" src=\"{source}\" title=\"{title}\" alt=\"{glyph}\" style=\"width:{size}px;height:{size}px;\"></span>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heart_only(glyph: &str) -> Option<EmoticonTag> {
        glyph.starts_with('\u{2764}').then(|| EmoticonTag {
            id: "heart".into(),
            title: "Heart".into(),
            image_url: "https://e/heart/default/{size}_f.png".into(),
        })
    }

    fn heart_span(size: u32) -> String {
        format!(
            "<span contenteditable=\"false\" title=\"Heart\" type=\"(heart)\" class=\"animated-emoticon-{size}-heart\" itemscope=\"\"><img itemscope=\"\" itemtype=\"http://schema.skype.com/Emoji\" itemid=\"heart\" src=\"https://e/heart/default/{size}_f.png\" title=\"Heart\" alt=\"❤️\" style=\"width:{size}px;height:{size}px;\"></span>"
        )
    }

    #[test]
    fn emoji_in_text_become_inline_composer_spans() {
        assert_eq!(
            emoticon_tags("<p>love ❤️ it 😀</p>", heart_only),
            format!("<p>love {} it 😀</p>", heart_span(20))
        );
    }

    #[test]
    fn emoji_only_messages_get_the_50_size() {
        assert_eq!(
            emoticon_tags("<p>❤️ ❤️</p>", heart_only),
            format!("<p>{} {}</p>", heart_span(50), heart_span(50))
        );
    }

    #[test]
    fn code_attributes_and_markup_stay_untouched() {
        let html = "<pre><code>❤️</code></pre><p><a href=\"x\" title=\"❤️\">❤️</a></p>";
        assert_eq!(
            emoticon_tags(html, heart_only),
            format!(
                "<pre><code>❤️</code></pre><p><a href=\"x\" title=\"❤️\">{}</a></p>",
                heart_span(20)
            )
        );
    }
}
