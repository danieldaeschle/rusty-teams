use scraper::{ElementRef, Html, Node};
use unicode_segmentation::UnicodeSegmentation;

pub const MAX_JUMBO_EMOJI: usize = 3;

const EMOJI_ITEMTYPE: &str = "schema.skype.com/Emoji";
const BLOCKING_ELEMENTS: [&str; 4] = ["table", "attachment", "blockquote", "pre"];
const IGNORED_CHARACTERS: [char; 5] = ['\u{a0}', '\u{200b}', '\u{200c}', '\u{200d}', '\u{feff}'];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JumboEmoji {
    pub id: Option<String>,
    pub glyph: String,
}

#[derive(Default)]
struct Scan {
    emoticons: Vec<JumboEmoji>,
    text: String,
    break_pending: bool,
    break_before_content: bool,
    blocked: bool,
}

impl Scan {
    fn content_follows(&mut self) {
        if self.break_pending {
            self.break_before_content = true;
        }
    }
}

/// The Teams emoji-only rule: 1 to 3 emoticons and nothing else, or 1 to 3 unicode emoji
/// without whitespace between them. Such a message shows big and without a bubble.
pub fn jumbo_emoji(html: &str) -> Option<Vec<JumboEmoji>> {
    let fragment = Html::parse_fragment(html);
    let mut scan = Scan::default();
    walk(fragment.root_element(), &mut scan);
    if scan.blocked || scan.break_before_content {
        return None;
    }
    let found = match (scan.emoticons.is_empty(), scan.text.trim().is_empty()) {
        (false, true) => scan.emoticons,
        (true, false) => unicode_emoji(scan.text.trim())?,
        _ => return None,
    };
    (1..=MAX_JUMBO_EMOJI)
        .contains(&found.len())
        .then_some(found)
}

fn walk(element: ElementRef<'_>, scan: &mut Scan) {
    for child in element.children() {
        if scan.blocked {
            return;
        }
        match child.value() {
            Node::Text(text) => {
                let visible: String = text
                    .chars()
                    .filter(|character| !IGNORED_CHARACTERS.contains(character))
                    .collect();
                if !visible.trim().is_empty() {
                    scan.content_follows();
                    scan.text.push_str(text);
                }
            }
            Node::Element(_) => {
                if let Some(child_element) = ElementRef::wrap(child) {
                    visit_element(child_element, scan);
                }
            }
            _ => {}
        }
    }
}

fn visit_element(element: ElementRef<'_>, scan: &mut Scan) {
    let value = element.value();
    let is_emoji_image = value.name() == "img"
        && value
            .attr("itemtype")
            .is_some_and(|itemtype| itemtype.contains(EMOJI_ITEMTYPE));
    match value.name() {
        "emoji" => {
            scan.content_follows();
            scan.emoticons.push(JumboEmoji {
                id: value.attr("id").map(str::to_owned),
                glyph: glyph(element),
            });
        }
        "img" if is_emoji_image => {
            scan.content_follows();
            scan.emoticons.push(JumboEmoji {
                id: value.attr("itemid").or(value.attr("id")).map(str::to_owned),
                glyph: glyph(element),
            });
        }
        "img" => scan.blocked = true,
        "br" => scan.break_pending = true,
        name if BLOCKING_ELEMENTS.contains(&name) => scan.blocked = true,
        _ => walk(element, scan),
    }
}

fn glyph(element: ElementRef<'_>) -> String {
    element
        .value()
        .attr("alt")
        .filter(|alt| !alt.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| element.text().collect())
}

fn unicode_emoji(text: &str) -> Option<Vec<JumboEmoji>> {
    text.graphemes(true)
        .map(|grapheme| {
            is_emoji_grapheme(grapheme).then(|| JumboEmoji {
                id: None,
                glyph: grapheme.to_owned(),
            })
        })
        .collect()
}

fn is_emoji_grapheme(grapheme: &str) -> bool {
    let mut characters = grapheme.chars();
    characters.next().is_some_and(is_pictographic)
        && characters.all(|character| {
            is_pictographic(character)
                || matches!(
                    character,
                    '\u{1f3fb}'..='\u{1f3ff}' | '\u{200d}' | '\u{fe0f}' | '\u{20e3}'
                )
        })
}

fn is_pictographic(character: char) -> bool {
    matches!(
        character,
        '\u{a9}'
            | '\u{ae}'
            | '\u{203c}'
            | '\u{2049}'
            | '\u{2122}'
            | '\u{2139}'
            | '\u{2194}'..='\u{21aa}'
            | '\u{231a}'..='\u{23ff}'
            | '\u{24c2}'
            | '\u{25aa}'..='\u{27bf}'
            | '\u{2934}'..='\u{2935}'
            | '\u{2b05}'..='\u{2b55}'
            | '\u{3030}'
            | '\u{303d}'
            | '\u{3297}'
            | '\u{3299}'
            | '\u{1f000}'..='\u{1f0ff}'
            | '\u{1f10d}'..='\u{1f1ad}'
            | '\u{1f201}'..='\u{1f3fa}'
            | '\u{1f400}'..='\u{1faff}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emoticon(id: &str, glyph: &str) -> JumboEmoji {
        JumboEmoji {
            id: Some(id.to_owned()),
            glyph: glyph.to_owned(),
        }
    }

    fn glyphs(found: Option<Vec<JumboEmoji>>) -> Option<Vec<String>> {
        found.map(|found| found.into_iter().map(|emoji| emoji.glyph).collect())
    }

    #[test]
    fn one_to_three_emoticons_with_whitespace_and_wrappers_are_jumbo() {
        let html = r#"<p><emoji id="smile" alt="🙂" title="Smile"></emoji>&nbsp;<img itemtype="http://schema.skype.com/Emoji" itemid="heart" alt="❤️" src="https://x/e.png"></p>"#;
        assert_eq!(
            jumbo_emoji(html),
            Some(vec![emoticon("smile", "🙂"), emoticon("heart", "❤️")])
        );
    }

    #[test]
    fn four_emoticons_text_quotes_tables_and_images_are_not_jumbo() {
        let four = r#"<emoji id="a" alt="🙂"></emoji>"#.repeat(4);
        assert_eq!(jumbo_emoji(&four), None);
        assert_eq!(
            jumbo_emoji(r#"hi <emoji id="smile" alt="🙂"></emoji>"#),
            None
        );
        assert_eq!(
            jumbo_emoji(r#"<attachment id="1"></attachment><emoji id="smile" alt="🙂"></emoji>"#),
            None
        );
        assert_eq!(
            jumbo_emoji(r#"<emoji id="smile" alt="🙂"></emoji><img src="https://x/photo.png">"#),
            None
        );
        assert_eq!(jumbo_emoji("<p></p>"), None);
    }

    #[test]
    fn a_break_before_the_next_emoji_ends_jumbo_but_a_trailing_break_does_not() {
        assert_eq!(
            jumbo_emoji(r#"<emoji id="a" alt="🙂"></emoji><br><emoji id="b" alt="🙂"></emoji>"#),
            None
        );
        assert_eq!(
            jumbo_emoji(r#"<emoji id="a" alt="🙂"></emoji><br>"#),
            Some(vec![emoticon("a", "🙂")])
        );
    }

    #[test]
    fn unicode_only_needs_one_to_three_emoji_without_whitespace() {
        assert_eq!(
            glyphs(jumbo_emoji("<p>😀👍🏽❤️</p>")),
            Some(vec!["😀".into(), "👍🏽".into(), "❤️".into()])
        );
        assert_eq!(glyphs(jumbo_emoji("<p>👨‍👩‍👧</p>")), Some(vec!["👨‍👩‍👧".into()]));
        assert_eq!(jumbo_emoji("<p>😀 😀</p>"), None);
        assert_eq!(jumbo_emoji("<p>😀😀😀😀</p>"), None);
        assert_eq!(jumbo_emoji("<p>ok 😀</p>"), None);
        assert_eq!(jumbo_emoji("<p>12</p>"), None);
    }
}
