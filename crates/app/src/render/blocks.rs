use std::ops::Range;

use teams_core::{FontSize, Span};

pub const IMAGE_PLACEHOLDER: &str = "[image]";
pub const CODE_PADDING: &str = "\u{2004}";
const URL_PREFIXES: [&str; 3] = ["https://", "http://", "www."];
const TRAILING_PUNCTUATION: &str = ".,;:!?'\"";
const BASE_FONT_PIXELS: f32 = 14.;
const SMALL_SCALE: f32 = 0.75;
const LARGE_SCALE: f32 = 1.5;
const SCRIPT_SCALE: f32 = 0.75;
const SUPERSCRIPT_RAISE: f32 = 0.35;
const SUBSCRIPT_DROP: f32 = 0.2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    Super,
    Sub,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StyleFlags {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub mention: bool,
    pub link: bool,
    pub strike: bool,
    pub underline: bool,
    pub script: Option<Script>,
    pub size: Option<FontSize>,
    pub color: Option<u32>,
    pub background: Option<u32>,
}

impl StyleFlags {
    pub fn is_plain(&self) -> bool {
        *self == StyleFlags::default()
    }

    pub fn is_flowed(&self) -> bool {
        self.script.is_some() || self.size.is_some()
    }

    pub fn baseline_raise(&self) -> f32 {
        match self.script {
            Some(Script::Super) => SUPERSCRIPT_RAISE,
            Some(Script::Sub) => -SUBSCRIPT_DROP,
            None => 0.,
        }
    }

    pub fn font_scale(&self) -> f32 {
        let size = match self.size {
            Some(FontSize::Small) => SMALL_SCALE,
            Some(FontSize::Large) => LARGE_SCALE,
            Some(FontSize::Pixels(pixels)) => f32::from(pixels) / BASE_FONT_PIXELS,
            Some(FontSize::Hidden) | None => 1.,
        };
        match self.script {
            Some(_) => size * SCRIPT_SCALE,
            None => size,
        }
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

    pub fn plain_text(&self) -> String {
        self.text.replace(CODE_PADDING, "")
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
    Image {
        url: String,
    },
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
            Block::Paragraph(inline) | Block::Heading { inline, .. } => Some(inline.plain_text()),
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
                            .map(Inline::plain_text)
                            .collect::<Vec<_>>()
                            .join(" | ")
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            Block::Quote(children) => Some(join(children)),
            Block::Reply(_) | Block::Rule | Block::Image { .. } => None,
        }
    }

    pub fn first_line(&self) -> Option<String> {
        match self {
            Block::Paragraph(inline) | Block::Heading { inline, .. } => Some(inline.plain_text()),
            Block::Code { code, .. } => Some(code.clone()),
            Block::List { items, .. } => items.first()?.iter().find_map(Block::first_line),
            Block::Table { rows, .. } => Some(rows.first()?.first()?.plain_text()),
            Block::Quote(children) => children.iter().find_map(Block::first_line),
            Block::Reply(_) | Block::Rule | Block::Image { .. } => None,
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
    layout_blocks_with(spans, true)
}

fn layout_blocks_with(spans: &[Span], split_images: bool) -> Vec<Block> {
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
                let inner = layout_blocks_with(children, false);
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
                    .map(|item| layout_blocks_with(item, false))
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
            Span::Image { hosted_content_url } if split_images => {
                flush(&mut current, &mut blocks, Block::Paragraph);
                blocks.push(Block::Image {
                    url: hosted_content_url.clone(),
                });
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
    let leading = inline.text.len() - inline.text.trim_start().len();
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
        let touches = |neighbour: Option<&Segment>| {
            neighbour.is_some_and(|neighbour| !neighbour.style.code && !neighbour.range.is_empty())
        };
        let previous = index
            .checked_sub(1)
            .and_then(|previous| inline.segments.get(previous));
        if segment.style.code && touches(previous) {
            padded.push(CODE_PADDING, StyleFlags::default(), None);
        }
        padded.push(text, segment.style, segment.link.as_deref());
        if segment.style.code && touches(inline.segments.get(index + 1)) {
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
            Span::Superscript(children) => append_inline(
                children,
                StyleFlags {
                    script: Some(Script::Super),
                    ..style
                },
                link,
                out,
            ),
            Span::Subscript(children) => append_inline(
                children,
                StyleFlags {
                    script: Some(Script::Sub),
                    ..style
                },
                link,
                out,
            ),
            Span::Sized(FontSize::Hidden, _) => {}
            Span::Sized(size, children) => append_inline(
                children,
                StyleFlags {
                    size: Some(*size),
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
    fn script_and_size_become_flags_with_a_font_scale() {
        let inline = paragraph_inline(&[
            Span::Superscript(vec![text("2")]),
            Span::Sized(FontSize::Large, vec![text("big")]),
            Span::Sized(FontSize::Small, vec![Span::Subscript(vec![text("s")])]),
            Span::Sized(FontSize::Pixels(21), vec![text("p")]),
            Span::Sized(FontSize::Hidden, vec![text("secret")]),
        ]);
        let styles: Vec<StyleFlags> = inline
            .segments
            .iter()
            .map(|segment| segment.style)
            .collect();
        assert_eq!(inline.text, "2bigsp");
        assert_eq!(styles[0].script, Some(Script::Super));
        assert_eq!(styles[0].font_scale(), 0.75);
        assert_eq!(styles[1].font_scale(), 1.5);
        assert_eq!(styles[2].script, Some(Script::Sub));
        assert_eq!(styles[2].font_scale(), 0.5625);
        assert_eq!(styles[3].font_scale(), 1.5);
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
    fn inline_code_gets_room_next_to_text() {
        let inline = paragraph_inline(&[
            text("run "),
            Span::Code("x".into()),
            text(". "),
            Span::Code("y".into()),
        ]);
        assert_eq!(inline.text, "run \u{2004}x\u{2004}. \u{2004}y");
        assert_eq!(inline.plain_text(), "run x. y");
    }

    fn image(url: &str) -> Span {
        Span::Image {
            hosted_content_url: url.into(),
        }
    }

    #[test]
    fn top_level_image_is_its_own_block() {
        assert_eq!(
            layout_blocks(&[image("u")]),
            vec![Block::Image { url: "u".into() }]
        );
    }

    #[test]
    fn image_splits_the_paragraph_at_its_position() {
        let blocks = layout_blocks(&[text("a "), image("u"), text(" b")]);
        assert_eq!(
            blocks,
            vec![
                Block::Paragraph(Inline::plain("a")),
                Block::Image { url: "u".into() },
                Block::Paragraph(Inline::plain("b")),
            ]
        );
    }

    #[test]
    fn image_inside_a_list_keeps_the_placeholder() {
        let blocks = layout_blocks(&[Span::List {
            ordered: false,
            start: 1,
            items: vec![vec![text("x "), image("u")]],
        }]);
        assert_eq!(
            blocks,
            vec![Block::List {
                ordered: false,
                start: 1,
                items: vec![vec![Block::Paragraph(Inline::plain("x [image]"))]],
            }]
        );
    }

    #[test]
    fn image_inside_a_quote_keeps_the_placeholder() {
        let blocks = layout_blocks(&[Span::BlockQuote(vec![image("u")])]);
        assert_eq!(
            blocks,
            vec![Block::Quote(vec![Block::Paragraph(Inline::plain(
                IMAGE_PLACEHOLDER
            ))])]
        );
    }

    #[test]
    fn image_blocks_have_no_text() {
        let block = Block::Image { url: "u".into() };
        assert_eq!(block.markdown(), None);
        assert_eq!(block.first_line(), None);
    }

    #[test]
    fn stripping_placeholders_keeps_the_text_around_them() {
        let blocks = layout_blocks(&[Span::List {
            ordered: false,
            start: 1,
            items: vec![vec![
                text("look "),
                Span::Bold(vec![text("here")]),
                Span::LineBreak,
                image("u"),
            ]],
        }]);
        let stripped = strip_image_placeholders(blocks);
        let Block::List { items, .. } = &stripped[0] else {
            panic!("list expected")
        };
        let Block::Paragraph(inline) = &items[0][0] else {
            panic!("paragraph expected")
        };
        assert_eq!(inline.text, "look here");
        assert!(inline.segments.last().unwrap().style.bold);
        let only_image = vec![Block::Paragraph(Inline::plain(IMAGE_PLACEHOLDER))];
        assert!(strip_image_placeholders(only_image).is_empty());
    }

    #[test]
    fn stripping_placeholders_keeps_image_blocks() {
        let blocks = vec![Block::Image { url: "u".into() }];
        assert_eq!(strip_image_placeholders(blocks.clone()), blocks);
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
