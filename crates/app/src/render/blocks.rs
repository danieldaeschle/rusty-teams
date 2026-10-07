use std::ops::Range;

use teams_core::Span;

pub const MENTION_PAD: char = '\u{2009}';
pub const IMAGE_PLACEHOLDER: &str = "[image]";
const URL_PREFIXES: [&str; 3] = ["https://", "http://", "www."];
const TRAILING_PUNCTUATION: &str = ".,;:!?'\"";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StyleFlags {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub mention: bool,
    pub link: bool,
}

impl StyleFlags {
    pub fn is_plain(&self) -> bool {
        *self == StyleFlags::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub range: Range<usize>,
    pub style: StyleFlags,
    pub link: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inline {
    pub text: String,
    pub segments: Vec<Segment>,
}

impl Inline {
    pub fn plain(text: &str) -> Self {
        let mut inline = Inline::default();
        inline.push(text, StyleFlags::default(), None);
        inline
    }

    fn push(&mut self, text: &str, style: StyleFlags, link: Option<&str>) {
        if text.is_empty() {
            return;
        }
        let start = self.text.len();
        self.text.push_str(text);
        let end = self.text.len();
        if let Some(last) = self.segments.last_mut()
            && last.style == style
            && last.link.as_deref() == link
            && last.range.end == start
        {
            last.range.end = end;
            return;
        }
        self.segments.push(Segment {
            range: start..end,
            style,
            link: link.map(str::to_owned),
        });
    }

    pub fn with_trailing_room(&self, width_in_spaces: usize) -> Inline {
        let mut padded = self.clone();
        padded.push(
            &"\u{2007}".repeat(width_in_spaces),
            StyleFlags::default(),
            None,
        );
        padded
    }

    pub fn is_blank(&self) -> bool {
        self.text.trim().is_empty()
    }

    fn trim_end(&mut self) {
        let trimmed = self.text.trim_end().len();
        self.text.truncate(trimmed);
        self.segments.retain_mut(|segment| {
            segment.range.end = segment.range.end.min(trimmed);
            segment.range.start < segment.range.end
        });
    }

    fn without(&self, needle: &str) -> Inline {
        let mut stripped = Inline::default();
        for segment in &self.segments {
            let text = self.text[segment.range.clone()].replace(needle, "");
            stripped.push(&text, segment.style, segment.link.as_deref());
        }
        stripped.trim_end();
        let leading = stripped.text.len() - stripped.text.trim_start().len();
        if leading > 0 {
            let mut trimmed = Inline::default();
            for segment in &stripped.segments {
                let text = stripped.text[segment.range.clone()].to_owned();
                let skip = leading.saturating_sub(segment.range.start).min(text.len());
                trimmed.push(&text[skip..], segment.style, segment.link.as_deref());
            }
            return trimmed;
        }
        stripped
    }

    pub fn links(&self) -> Vec<(Range<usize>, String)> {
        self.segments
            .iter()
            .filter_map(|segment| Some((segment.range.clone(), segment.link.clone()?)))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Paragraph(Inline),
    Code(String),
    Quote(Vec<Block>),
    ListItem(Inline),
}

pub fn strip_image_placeholders(blocks: Vec<Block>) -> Vec<Block> {
    blocks
        .into_iter()
        .filter_map(|block| match block {
            Block::Paragraph(inline) => {
                let stripped = inline.without(IMAGE_PLACEHOLDER);
                (!stripped.is_blank()).then_some(Block::Paragraph(stripped))
            }
            Block::Quote(children) => {
                let children = strip_image_placeholders(children);
                (!children.is_empty()).then_some(Block::Quote(children))
            }
            other => Some(other),
        })
        .collect()
}

pub fn layout_blocks(spans: &[Span]) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut current = Inline::default();
    for span in spans {
        match span {
            Span::CodeBlock { code, .. } => {
                flush(&mut current, &mut blocks, Block::Paragraph);
                blocks.push(Block::Code(code.trim_end_matches('\n').to_owned()));
            }
            Span::Quote(children) => {
                flush(&mut current, &mut blocks, Block::Paragraph);
                let inner = layout_blocks(children);
                if !inner.is_empty() {
                    blocks.push(Block::Quote(inner));
                }
            }
            Span::ListItem(children) => {
                flush(&mut current, &mut blocks, Block::Paragraph);
                let mut item = Inline::default();
                append_inline(children, StyleFlags::default(), None, &mut item);
                item.trim_end();
                blocks.push(Block::ListItem(item));
            }
            other => append_inline(
                std::slice::from_ref(other),
                StyleFlags::default(),
                None,
                &mut current,
            ),
        }
    }
    flush(&mut current, &mut blocks, Block::Paragraph);
    blocks
}

fn flush(current: &mut Inline, blocks: &mut Vec<Block>, make: fn(Inline) -> Block) {
    let mut inline = std::mem::take(current);
    inline.trim_end();
    let leading = inline.text.len() - inline.text.trim_start_matches('\n').len();
    if leading > 0 {
        inline.text.drain(..leading);
        inline.segments.retain_mut(|segment| {
            segment.range.start = segment.range.start.saturating_sub(leading);
            segment.range.end = segment.range.end.saturating_sub(leading);
            segment.range.start < segment.range.end
        });
    }
    if !inline.is_blank() {
        blocks.push(make(inline));
    }
}

fn append_inline(spans: &[Span], style: StyleFlags, link: Option<&str>, out: &mut Inline) {
    for span in spans {
        match span {
            Span::Text(text) if link.is_none() && !style.code => {
                push_linkified(text, style, out);
            }
            Span::Text(text) => out.push(text, style, link),
            Span::Bold(children) => append_inline(
                children,
                StyleFlags {
                    bold: true,
                    ..style
                },
                link,
                out,
            ),
            Span::Italic(children) => append_inline(
                children,
                StyleFlags {
                    italic: true,
                    ..style
                },
                link,
                out,
            ),
            Span::Code(text) => out.push(
                text,
                StyleFlags {
                    code: true,
                    ..style
                },
                link,
            ),
            Span::CodeBlock { code, .. } => out.push(
                code.trim_end_matches('\n'),
                StyleFlags {
                    code: true,
                    ..style
                },
                link,
            ),
            Span::Link { url, children } => append_inline(
                children,
                StyleFlags {
                    link: true,
                    ..style
                },
                Some(url.as_str()),
                out,
            ),
            Span::Mention { name } => {
                let label = format!(
                    "{MENTION_PAD}@{}{MENTION_PAD}",
                    name.trim_start_matches('@')
                );
                out.push(
                    &label,
                    StyleFlags {
                        mention: true,
                        ..style
                    },
                    link,
                );
            }
            Span::LineBreak => out.push("\n", style, link),
            Span::ListItem(children) | Span::Quote(children) => {
                append_inline(children, style, link, out);
                out.push("\n", style, link);
            }
            Span::Image { .. } => out.push(IMAGE_PLACEHOLDER, style, link),
        }
    }
}

fn push_linkified(text: &str, style: StyleFlags, out: &mut Inline) {
    let mut rest = text;
    while let Some((start, end)) = find_url(rest) {
        out.push(&rest[..start], style, None);
        let visible = &rest[start..end];
        let target = if visible.starts_with("www.") {
            format!("https://{visible}")
        } else {
            visible.to_owned()
        };
        out.push(
            visible,
            StyleFlags {
                link: true,
                ..style
            },
            Some(&target),
        );
        rest = &rest[end..];
    }
    out.push(rest, style, None);
}

fn find_url(text: &str) -> Option<(usize, usize)> {
    let mut previous_is_boundary = true;
    for (start, character) in text.char_indices() {
        if previous_is_boundary
            && let Some(prefix) = URL_PREFIXES
                .iter()
                .find(|prefix| text[start..].starts_with(**prefix))
        {
            let length = text[start..]
                .find(char::is_whitespace)
                .unwrap_or(text.len() - start);
            let end = start + trimmed_url_length(&text[start..start + length]);
            if end > start + prefix.len() {
                return Some((start, end));
            }
        }
        previous_is_boundary = !character.is_alphanumeric();
    }
    None
}

fn trimmed_url_length(candidate: &str) -> usize {
    let mut url = candidate;
    while let Some(last) = url.chars().next_back() {
        let opener = match last {
            ')' => Some('('),
            ']' => Some('['),
            '}' => Some('{'),
            _ => None,
        };
        let trimmable = match opener {
            Some(opener) => url.matches(last).count() > url.matches(opener).count(),
            None => TRAILING_PUNCTUATION.contains(last),
        };
        if !trimmable {
            break;
        }
        url = &url[..url.len() - last.len_utf8()];
    }
    url.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Span {
        Span::Text(value.to_owned())
    }

    #[test]
    fn plain_text_is_one_paragraph() {
        let blocks = layout_blocks(&[text("hello ")]);
        assert_eq!(blocks, vec![Block::Paragraph(Inline::plain("hello"))]);
    }

    #[test]
    fn bold_and_italic_nest_into_flags() {
        let blocks = layout_blocks(&[
            text("a "),
            Span::Bold(vec![Span::Italic(vec![text("b")])]),
            text(" c"),
        ]);
        let Block::Paragraph(inline) = &blocks[0] else {
            panic!("paragraph expected")
        };
        assert_eq!(inline.text, "a b c");
        assert_eq!(inline.segments.len(), 3);
        assert!(inline.segments[1].style.bold && inline.segments[1].style.italic);
        assert_eq!(inline.segments[1].range, 2..3);
    }

    #[test]
    fn link_keeps_url_and_range() {
        let blocks = layout_blocks(&[
            text("see "),
            Span::Link {
                url: "https://example.com".into(),
                children: vec![text("here")],
            },
        ]);
        let Block::Paragraph(inline) = &blocks[0] else {
            panic!("paragraph expected")
        };
        assert_eq!(
            inline.links(),
            vec![(4..8, "https://example.com".to_owned())]
        );
    }

    fn paragraph_inline(spans: &[Span]) -> Inline {
        let Block::Paragraph(inline) = layout_blocks(spans).remove(0) else {
            panic!("paragraph expected")
        };
        inline
    }

    #[test]
    fn plain_url_at_end_becomes_link() {
        let inline = paragraph_inline(&[text("see https://example.com/a")]);
        assert_eq!(
            inline.links(),
            vec![(4..25, "https://example.com/a".to_owned())]
        );
        assert!(inline.segments[1].style.link);
    }

    #[test]
    fn trailing_period_stays_outside_url() {
        let inline = paragraph_inline(&[text("go to http://example.com. Then")]);
        assert_eq!(
            inline.links(),
            vec![(6..24, "http://example.com".to_owned())]
        );
    }

    #[test]
    fn balanced_parentheses_stay_in_url() {
        let inline = paragraph_inline(&[text("https://example.com/Rust_(language)")]);
        assert_eq!(inline.links()[0].1, "https://example.com/Rust_(language)");
        let wrapped = paragraph_inline(&[text("(see https://example.com)")]);
        assert_eq!(wrapped.links()[0].1, "https://example.com");
    }

    #[test]
    fn www_url_gets_https_target() {
        let inline = paragraph_inline(&[text("www.example.com!")]);
        assert_eq!(inline.text, "www.example.com!");
        assert_eq!(
            inline.links(),
            vec![(0..15, "https://www.example.com".to_owned())]
        );
    }

    #[test]
    fn text_without_url_has_no_links() {
        let inline = paragraph_inline(&[text("no link, just www. and https:// here")]);
        assert!(inline.links().is_empty());
    }

    #[test]
    fn url_inside_link_stays_one_link() {
        let inline = paragraph_inline(&[Span::Link {
            url: "https://target.example".into(),
            children: vec![text("https://shown.example")],
        }]);
        assert_eq!(
            inline.links(),
            vec![(0..21, "https://target.example".to_owned())]
        );
    }

    #[test]
    fn mention_gets_an_at_sign_once() {
        let blocks = layout_blocks(&[
            Span::Mention { name: "Ada".into() },
            text(" "),
            Span::Mention {
                name: "@Bob".into(),
            },
        ]);
        let Block::Paragraph(inline) = &blocks[0] else {
            panic!("paragraph expected")
        };
        assert_eq!(inline.text, "\u{2009}@Ada\u{2009} \u{2009}@Bob");
        assert!(inline.segments[0].style.mention);
    }

    #[test]
    fn code_block_splits_paragraphs() {
        let blocks = layout_blocks(&[
            text("before"),
            Span::CodeBlock {
                language: None,
                code: "let x = 1;\n".into(),
            },
            text("after"),
        ]);
        assert_eq!(
            blocks,
            vec![
                Block::Paragraph(Inline::plain("before")),
                Block::Code("let x = 1;".into()),
                Block::Paragraph(Inline::plain("after")),
            ]
        );
    }

    #[test]
    fn list_items_and_quotes_are_blocks() {
        let blocks = layout_blocks(&[
            Span::ListItem(vec![text("one")]),
            Span::ListItem(vec![text("two")]),
            Span::Quote(vec![text("quoted")]),
        ]);
        assert_eq!(
            blocks,
            vec![
                Block::ListItem(Inline::plain("one")),
                Block::ListItem(Inline::plain("two")),
                Block::Quote(vec![Block::Paragraph(Inline::plain("quoted"))]),
            ]
        );
    }

    #[test]
    fn image_becomes_placeholder() {
        let blocks = layout_blocks(&[Span::Image {
            hosted_content_url: "u".into(),
        }]);
        assert_eq!(
            blocks,
            vec![Block::Paragraph(Inline::plain(IMAGE_PLACEHOLDER))]
        );
    }

    #[test]
    fn stripping_placeholders_keeps_the_text_around_them() {
        let blocks = layout_blocks(&[
            text("look "),
            Span::Bold(vec![text("here")]),
            Span::LineBreak,
            Span::Image {
                hosted_content_url: "u".into(),
            },
        ]);
        let stripped = strip_image_placeholders(blocks);
        let Block::Paragraph(inline) = &stripped[0] else {
            panic!("paragraph expected")
        };
        assert_eq!(inline.text, "look here");
        assert!(inline.segments.last().unwrap().style.bold);
        let only_image = layout_blocks(&[Span::Image {
            hosted_content_url: "u".into(),
        }]);
        assert!(strip_image_placeholders(only_image).is_empty());
    }

    #[test]
    fn line_break_stays_inside_paragraph() {
        let blocks = layout_blocks(&[text("a"), Span::LineBreak, text("b")]);
        assert_eq!(blocks, vec![Block::Paragraph(Inline::plain("a\nb"))]);
    }

    #[test]
    fn empty_input_has_no_blocks() {
        assert!(layout_blocks(&[]).is_empty());
        assert!(layout_blocks(&[Span::LineBreak, text("  ")]).is_empty());
    }
}
