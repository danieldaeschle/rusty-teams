use std::ops::Range;

use teams_core::Span;

pub const IMAGE_PLACEHOLDER: &str = "[image]";
pub const CODE_PADDING: &str = "\u{200a}";
const URL_PREFIXES: [&str; 3] = ["https://", "http://", "www."];
const TRAILING_PUNCTUATION: &str = ".,;:!?'\"";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StyleFlags {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub mention: bool,
    pub link: bool,
    pub strike: bool,
    pub underline: bool,
    pub color: Option<u32>,
    pub background: Option<u32>,
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
    Heading {
        level: u8,
        inline: Inline,
    },
    Code {
        language: Option<String>,
        code: String,
    },
    Reply(Vec<Block>),
    Quote(Vec<Block>),
    List {
        ordered: bool,
        start: u32,
        items: Vec<Vec<Block>>,
    },
    Table {
        header: bool,
        rows: Vec<Vec<Inline>>,
    },
    Rule,
}

impl Block {
    pub fn markdown(&self) -> Option<String> {
        let join = |blocks: &[Block]| {
            blocks
                .iter()
                .filter_map(Block::markdown)
                .collect::<Vec<_>>()
                .join("\n")
        };
        match self {
            Block::Paragraph(inline) | Block::Heading { inline, .. } => Some(inline.text.clone()),
            Block::Code { code, .. } => Some(format!("```\n{code}\n```")),
            Block::List {
                ordered,
                start,
                items,
            } => Some(
                items
                    .iter()
                    .enumerate()
                    .map(|(offset, item)| match ordered {
                        true => format!("{}. {}", *start as usize + offset, join(item)),
                        false => format!("- {}", join(item)),
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            Block::Table { rows, .. } => Some(
                rows.iter()
                    .map(|row| {
                        row.iter()
                            .map(|cell| cell.text.as_str())
                            .collect::<Vec<_>>()
                            .join(" | ")
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            Block::Quote(children) => Some(join(children)),
            Block::Reply(_) | Block::Rule => None,
        }
    }

    pub fn first_line(&self) -> Option<String> {
        match self {
            Block::Paragraph(inline) | Block::Heading { inline, .. } => Some(inline.text.clone()),
            Block::Code { code, .. } => Some(code.clone()),
            Block::List { items, .. } => items.first()?.iter().find_map(Block::first_line),
            Block::Table { rows, .. } => Some(rows.first()?.first()?.text.clone()),
            Block::Quote(children) => children.iter().find_map(Block::first_line),
            Block::Reply(_) | Block::Rule => None,
        }
    }

    pub fn last_inline_mut(&mut self) -> Option<&mut Inline> {
        match self {
            Block::Paragraph(inline) => Some(inline),
            Block::List { items, .. } => items.last_mut()?.last_mut()?.last_inline_mut(),
            _ => None,
        }
    }
}

pub fn strip_image_placeholders(blocks: Vec<Block>) -> Vec<Block> {
    blocks
        .into_iter()
        .filter_map(|block| match block {
            Block::Paragraph(inline) => {
                let stripped = inline.without(IMAGE_PLACEHOLDER);
                (!stripped.is_blank()).then_some(Block::Paragraph(stripped))
            }
            Block::Reply(children) => {
                let children = strip_image_placeholders(children);
                (!children.is_empty()).then_some(Block::Reply(children))
            }
            Block::Quote(children) => {
                let children = strip_image_placeholders(children);
                (!children.is_empty()).then_some(Block::Quote(children))
            }
            Block::List {
                ordered,
                start,
                items,
            } => {
                let items: Vec<Vec<Block>> = items
                    .into_iter()
                    .map(strip_image_placeholders)
                    .filter(|item| !item.is_empty())
                    .collect();
                (!items.is_empty()).then_some(Block::List {
                    ordered,
                    start,
                    items,
                })
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
            Span::CodeBlock { language, code } => {
                flush(&mut current, &mut blocks, Block::Paragraph);
                blocks.push(Block::Code {
                    language: language.clone(),
                    code: code.trim_end_matches('\n').to_owned(),
                });
            }
            Span::Quote(children) | Span::BlockQuote(children) => {
                flush(&mut current, &mut blocks, Block::Paragraph);
                let inner = layout_blocks(children);
                if !inner.is_empty() {
                    blocks.push(match span {
                        Span::Quote(_) => Block::Reply(inner),
                        _ => Block::Quote(inner),
                    });
                }
            }
            Span::List {
                ordered,
                start,
                items,
            } => {
                flush(&mut current, &mut blocks, Block::Paragraph);
                let items: Vec<Vec<Block>> = items
                    .iter()
                    .map(|item| layout_blocks(item))
                    .filter(|item| !item.is_empty())
                    .collect();
                if !items.is_empty() {
                    blocks.push(Block::List {
                        ordered: *ordered,
                        start: *start,
                        items,
                    });
                }
            }
            Span::Table { header, rows } => {
                flush(&mut current, &mut blocks, Block::Paragraph);
                let rows: Vec<Vec<Inline>> = rows
                    .iter()
                    .map(|row| row.iter().map(|cell| inline_of(cell)).collect())
                    .collect();
                if !rows.is_empty() {
                    blocks.push(Block::Table {
                        header: *header,
                        rows,
                    });
                }
            }
            Span::Heading { level, children } => {
                flush(&mut current, &mut blocks, Block::Paragraph);
                let inline = inline_of(children);
                if !inline.is_blank() {
                    blocks.push(Block::Heading {
                        level: *level,
                        inline,
                    });
                }
            }
            Span::Rule => {
                flush(&mut current, &mut blocks, Block::Paragraph);
                blocks.push(Block::Rule);
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

fn inline_of(spans: &[Span]) -> Inline {
    let mut inline = Inline::default();
    append_inline(spans, StyleFlags::default(), None, &mut inline);
    inline.trim_end();
    pad_code(inline)
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
        blocks.push(make(pad_code(inline)));
    }
}

fn pad_code(inline: Inline) -> Inline {
    if !inline.segments.iter().any(|segment| segment.style.code) {
        return inline;
    }
    let mut padded = Inline::default();
    for (index, segment) in inline.segments.iter().enumerate() {
        let text = &inline.text[segment.range.clone()];
        let touches = |neighbour: Option<&Segment>, at_end: bool| {
            neighbour.is_some_and(|neighbour| {
                let neighbour_text = &inline.text[neighbour.range.clone()];
                let edge = if at_end {
                    neighbour_text.chars().next()
                } else {
                    neighbour_text.chars().next_back()
                };
                !neighbour.style.code && edge.is_some_and(|edge| !edge.is_whitespace())
            })
        };
        let previous = index
            .checked_sub(1)
            .and_then(|previous| inline.segments.get(previous));
        if segment.style.code && touches(previous, false) {
            padded.push(CODE_PADDING, StyleFlags::default(), None);
        }
        padded.push(text, segment.style, segment.link.as_deref());
        if segment.style.code && touches(inline.segments.get(index + 1), true) {
            padded.push(CODE_PADDING, StyleFlags::default(), None);
        }
    }
    padded
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
            Span::Strike(children) => append_inline(
                children,
                StyleFlags {
                    strike: true,
                    ..style
                },
                link,
                out,
            ),
            Span::Underline(children) => append_inline(
                children,
                StyleFlags {
                    underline: true,
                    ..style
                },
                link,
                out,
            ),
            Span::Colored {
                color,
                background,
                children,
            } => append_inline(
                children,
                StyleFlags {
                    color: color.or(style.color),
                    background: background.or(style.background),
                    ..style
                },
                link,
                out,
            ),
            Span::Heading { children, .. } => {
                append_inline(
                    children,
                    StyleFlags {
                        bold: true,
                        ..style
                    },
                    link,
                    out,
                );
                out.push("\n", style, link);
            }
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
            Span::Mention { name, .. } => {
                let label = format!("@{}", name.trim_start_matches('@'));
                out.push(
                    &label,
                    StyleFlags {
                        mention: true,
                        ..style
                    },
                    link,
                );
            }
            Span::LineBreak | Span::Rule => out.push("\n", style, link),
            Span::Quote(children) | Span::BlockQuote(children) => {
                append_inline(children, style, link, out);
                out.push("\n", style, link);
            }
            Span::List { items, .. } => {
                for item in items {
                    append_inline(item, style, link, out);
                    out.push("\n", style, link);
                }
            }
            Span::Table { rows, .. } => {
                for row in rows {
                    for (index, cell) in row.iter().enumerate() {
                        if index > 0 {
                            out.push(" ", style, link);
                        }
                        append_inline(cell, style, link, out);
                    }
                    out.push("\n", style, link);
                }
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
            Span::Mention {
                name: "Ada".into(),
                id: None,
            },
            text(" "),
            Span::Mention {
                name: "@Bob".into(),
                id: None,
            },
        ]);
        let Block::Paragraph(inline) = &blocks[0] else {
            panic!("paragraph expected")
        };
        assert_eq!(inline.text, "@Ada @Bob");
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
                Block::Code {
                    language: None,
                    code: "let x = 1;".into()
                },
                Block::Paragraph(Inline::plain("after")),
            ]
        );
    }

    #[test]
    fn lists_nest_and_quotes_are_blocks() {
        let blocks = layout_blocks(&[
            Span::List {
                ordered: true,
                start: 3,
                items: vec![
                    vec![text("one")],
                    vec![
                        text("two"),
                        Span::List {
                            ordered: false,
                            start: 1,
                            items: vec![vec![text("inner")]],
                        },
                    ],
                ],
            },
            Span::Quote(vec![text("quoted")]),
        ]);
        assert_eq!(
            blocks,
            vec![
                Block::List {
                    ordered: true,
                    start: 3,
                    items: vec![
                        vec![Block::Paragraph(Inline::plain("one"))],
                        vec![
                            Block::Paragraph(Inline::plain("two")),
                            Block::List {
                                ordered: false,
                                start: 1,
                                items: vec![vec![Block::Paragraph(Inline::plain("inner"))]],
                            },
                        ],
                    ],
                },
                Block::Reply(vec![Block::Paragraph(Inline::plain("quoted"))]),
            ]
        );
    }

    #[test]
    fn heading_rule_and_table_are_blocks() {
        let blocks = layout_blocks(&[
            Span::Heading {
                level: 2,
                children: vec![text("Title")],
            },
            Span::Rule,
            Span::Table {
                header: true,
                rows: vec![vec![vec![text("a")], vec![text("b")]]],
            },
        ]);
        assert_eq!(
            blocks,
            vec![
                Block::Heading {
                    level: 2,
                    inline: Inline::plain("Title")
                },
                Block::Rule,
                Block::Table {
                    header: true,
                    rows: vec![vec![Inline::plain("a"), Inline::plain("b")]],
                },
            ]
        );
    }

    #[test]
    fn strike_underline_and_color_become_flags() {
        let inline = paragraph_inline(&[
            Span::Strike(vec![text("old")]),
            Span::Underline(vec![text("u")]),
            Span::Colored {
                color: Some(0xff0000),
                background: Some(0xffff00),
                children: vec![text("c")],
            },
        ]);
        assert!(inline.segments[0].style.strike);
        assert!(inline.segments[1].style.underline);
        assert_eq!(inline.segments[2].style.color, Some(0xff0000));
        assert_eq!(inline.segments[2].style.background, Some(0xffff00));
    }

    #[test]
    fn inline_code_gets_room_only_next_to_text() {
        let inline = paragraph_inline(&[
            text("run "),
            Span::Code("x".into()),
            text(". "),
            Span::Code("y".into()),
        ]);
        assert_eq!(inline.text, "run x\u{200a}. y");
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
