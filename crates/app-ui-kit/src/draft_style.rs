use std::ops::Range;

use gpui_kit::component::input::{RangeDecoration, RangeDecorationStyle, TextDecoration};
use gpui_kit::*;
use teams_core::{Draft, FontSize, LineKind, MarkKind, SizeStep};

use crate::render::blocks::{Script, StyleFlags};
use crate::render::elements::{highlight_fill, readable_on, token_style};
use crate::render::syntax;
use app_base::theme;

const CODE_PILL_RADIUS: f32 = 4.;
const CODE_BLOCK_RADIUS: f32 = 6.;
const QUOTE_BAR_WIDTH: f32 = 3.;
const QUOTE_INDENT: f32 = 12.;
/// Room before code lines: the line numbers sit in it.
pub const CODE_INDENT: f32 = 36.;
const LIST_LEVEL_INDENT: f32 = 18.;
const HEADING_SCALES: [f32; 3] = [1.6, 1.35, 1.15];
const INDENT_STEP: f32 = teams_core::INDENT_PIXELS as f32;
const HIGHLIGHT_RADIUS: f32 = 3.;

/// How the composer paints a draft: text styles, fills and the list markers to hang under.
pub struct DraftStyle {
    pub text: Vec<TextDecoration>,
    pub ranges: Vec<RangeDecoration>,
    pub markers: Vec<Range<usize>>,
    pub indents: Vec<(usize, Pixels)>,
    /// Code line numbers, each before the start of its line's content.
    pub labels: Vec<(usize, SharedString)>,
}

fn line_indent(kind: &LineKind) -> f32 {
    match kind {
        LineKind::Text | LineKind::Heading(_) | LineKind::Table(_) | LineKind::Rule => 0.,
        LineKind::Indent(level) => INDENT_STEP * f32::from(*level),
        LineKind::Quote => QUOTE_INDENT,
        LineKind::Code(_) => CODE_INDENT,
        LineKind::Bullet(depth) | LineKind::Numbered(depth) => {
            LIST_LEVEL_INDENT * f32::from(*depth)
        }
    }
}

fn color(color: Hsla) -> HighlightStyle {
    HighlightStyle {
        color: Some(color),
        ..Default::default()
    }
}

fn underline(color: Option<Hsla>) -> UnderlineStyle {
    UnderlineStyle {
        thickness: px(1.),
        color,
        wavy: false,
    }
}

fn mark_style(kind: &MarkKind) -> HighlightStyle {
    let mut style = HighlightStyle::default();
    match kind {
        MarkKind::Bold => style.font_weight = Some(FontWeight::BOLD),
        MarkKind::Italic => style.font_style = Some(FontStyle::Italic),
        MarkKind::Underline => style.underline = Some(underline(None)),
        MarkKind::Strike => {
            style.color = Some(theme::text_muted());
            style.strikethrough = Some(StrikethroughStyle {
                thickness: px(1.),
                color: None,
            });
        }
        MarkKind::Color(value) => {
            style.color = Some(readable_on(*value, theme::surface_raised()));
        }
        MarkKind::Superscript
        | MarkKind::Subscript
        | MarkKind::Size(_)
        | MarkKind::Highlight(_) => {}
        MarkKind::Code => style.color = Some(theme::text_strong()),
        MarkKind::Link(_) => {
            style.color = Some(theme::accent_text());
            style.underline = Some(underline(Some(theme::accent_text())));
        }
    }
    style
}

fn size_flags(kind: &MarkKind) -> StyleFlags {
    let mut flags = StyleFlags::default();
    match kind {
        MarkKind::Superscript => flags.script = Some(Script::Super),
        MarkKind::Subscript => flags.script = Some(Script::Sub),
        MarkKind::Size(SizeStep::Small) => flags.size = Some(FontSize::Small),
        MarkKind::Size(SizeStep::Large) => flags.size = Some(FontSize::Large),
        _ => {}
    }
    flags
}

fn heading_decoration(content: Range<usize>, level: u8) -> TextDecoration {
    let scale = HEADING_SCALES[usize::from(level.clamp(1, 3) - 1)];
    let style = HighlightStyle {
        font_weight: Some(FontWeight::BOLD),
        color: Some(theme::text_strong()),
        ..Default::default()
    };
    TextDecoration::new(content, style).with_font_scale(scale)
}

/// From the start of the first line of `lines` to the end of the last; Block and Bar cover
/// every line starting inside it, ends included, so empty lines count too.
fn block_range(draft: &Draft, lines: Range<usize>) -> Range<usize> {
    let line_ranges = draft.line_ranges();
    line_ranges[lines.start].start..line_ranges[lines.end - 1].end
}

/// A code line cut where its tokens start and end, so no two decorations overlap: the
/// input does not say which of two overlapping ones in a collection wins.
fn code_pieces(
    line: Range<usize>,
    tokens: &[(Range<usize>, HighlightStyle)],
) -> impl Iterator<Item = (Range<usize>, HighlightStyle)> {
    let plain = color(theme::text_strong());
    let mut pieces = Vec::new();
    let mut at = line.start;
    for (range, style) in tokens {
        let range = range.start.max(line.start)..range.end.min(line.end);
        if range.start >= range.end || range.start < at {
            continue;
        }
        if at < range.start {
            pieces.push((at..range.start, plain));
        }
        let mut style = *style;
        style.color = style.color.or(plain.color);
        pieces.push((range.clone(), style));
        at = range.end;
    }
    if at < line.end {
        pieces.push((at..line.end, plain));
    }
    pieces.into_iter()
}

pub fn draft_style(draft: &Draft, mono: SharedString) -> DraftStyle {
    let mut text = Vec::new();
    let mut ranges = Vec::new();
    let mut markers = Vec::new();
    let indents = draft
        .line_ranges()
        .into_iter()
        .zip(draft.lines())
        .map(|(line, kind)| (line.start, px(line_indent(kind))))
        .filter(|(_, indent)| *indent > px(0.))
        .collect();
    for mark in draft.marks() {
        let mut decoration = TextDecoration::new(mark.range.clone(), mark_style(&mark.kind));
        let flags = size_flags(&mark.kind);
        if flags.is_flowed() {
            decoration = decoration.with_font_scale(flags.font_scale());
            if flags.baseline_raise() != 0. {
                decoration = decoration.with_baseline_shift(flags.baseline_raise());
            }
        }
        if let MarkKind::Highlight(value) = mark.kind {
            ranges.push(
                RangeDecoration::new(mark.range.clone())
                    .with_style(RangeDecorationStyle::Pill)
                    .with_color(highlight_fill(value))
                    .with_radius(px(HIGHLIGHT_RADIUS)),
            );
        }
        if mark.kind == MarkKind::Code {
            text.push(decoration.with_font_family(mono.clone()));
            ranges.push(
                RangeDecoration::new(mark.range.clone())
                    .with_style(RangeDecorationStyle::Pill)
                    .with_color(theme::inline_code_fill(false))
                    .with_border(theme::inline_code_border())
                    .with_radius(px(CODE_PILL_RADIUS)),
            );
        } else {
            text.push(decoration);
        }
    }
    let mut labels = Vec::new();
    let kinds = draft.lines();
    let mut index = 0;
    while index < kinds.len() {
        let kind = &kinds[index];
        let run_end = index
            + kinds[index..]
                .iter()
                .take_while(|other| *other == kind)
                .count();
        // From after the header mark, so line one reads as a line start.
        let tokens: Vec<(Range<usize>, HighlightStyle)> = match kind {
            LineKind::Code(Some(language)) => {
                let block =
                    draft.content_range(index).start..block_range(draft, index..run_end).end;
                syntax::highlight(&draft.text()[block.clone()], language)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(range, token)| {
                        (
                            block.start + range.start..block.start + range.end,
                            token_style(token),
                        )
                    })
                    .collect()
            }
            _ => Vec::new(),
        };
        for line in index..run_end {
            let marker = draft.marker_range(line);
            let content = draft.content_range(line);
            if matches!(kind, LineKind::Code(_)) {
                labels.push((content.start, (line - index + 1).to_string().into()));
            }
            // A code block's header sits on its own row, so it hangs nothing.
            if !marker.is_empty() && !matches!(kind, LineKind::Code(_)) {
                markers.push(marker.clone());
                if kind.is_list() {
                    text.push(TextDecoration::new(marker, color(theme::text_soft())));
                }
            }
            if content.is_empty() {
                continue;
            }
            match kind {
                LineKind::Quote => {
                    text.push(TextDecoration::new(content, color(theme::text_soft())))
                }
                LineKind::Code(_) => {
                    text.extend(code_pieces(content, &tokens).map(|(range, style)| {
                        TextDecoration::new(range, style).with_font_family(mono.clone())
                    }))
                }
                LineKind::Heading(level) => text.push(heading_decoration(content, *level)),
                _ => {}
            }
        }
        let block = match kind {
            LineKind::Code(_) => Some(
                RangeDecoration::new(block_range(draft, index..run_end))
                    .with_style(RangeDecorationStyle::Block)
                    .with_color(theme::code_surface())
                    .with_border(theme::code_frame())
                    .with_radius(px(CODE_BLOCK_RADIUS)),
            ),
            LineKind::Quote => Some(
                RangeDecoration::new(block_range(draft, index..run_end))
                    .with_style(RangeDecorationStyle::Bar)
                    .with_color(theme::border_strong())
                    .with_radius(px(QUOTE_BAR_WIDTH)),
            ),
            _ => None,
        };
        ranges.extend(block);
        index = run_end;
    }
    DraftStyle {
        text,
        ranges,
        markers,
        indents,
        labels,
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::component::input::RangeDecorationStyle;
    use teams_core::{Draft, DraftLine, LineKind};

    use super::{block_range, draft_style};

    fn line(kind: LineKind, content: &str) -> DraftLine {
        DraftLine {
            kind,
            number: None,
            content: content.into(),
            marks: Vec::new(),
        }
    }

    #[test]
    fn list_markers_hang_and_code_gets_a_pill() {
        let draft = Draft::from_markdown("- one `x`\n```\nfn\n```");
        let style = draft_style(&draft, "Mono".into());
        assert_eq!(style.markers, [draft.marker_range(0)]);
        assert_eq!(
            style
                .ranges
                .iter()
                .map(|range| range.style())
                .collect::<Vec<_>>(),
            [RangeDecorationStyle::Pill, RangeDecorationStyle::Block]
        );
    }

    #[test]
    fn nesting_quotes_and_code_become_line_indents() {
        let draft = Draft::from_markdown("- a\n  - b\n> q\n```\nx\n```");
        let style = draft_style(&draft, "Mono".into());
        let starts = draft.line_ranges();
        assert_eq!(
            style.indents,
            [
                (starts[1].start, gpui_kit::px(18.)),
                (starts[2].start, gpui_kit::px(12.)),
                (starts[3].start, gpui_kit::px(36.)),
            ]
        );
        assert!(!draft.text().contains('\u{2003}'));
    }

    #[test]
    fn a_code_block_covers_its_empty_last_line_before_text() {
        let draft = Draft::from_lines(&[
            line(LineKind::Code(None), "x"),
            line(LineKind::Code(None), ""),
            line(LineKind::Text, "after"),
        ]);
        let block = block_range(&draft, 0..2);
        let empty_line_start = draft.line_ranges()[1].start;
        assert!(block.contains(&empty_line_start) || block.end == empty_line_start);
        assert!(block.end < draft.line_ranges()[2].start);
    }

    #[test]
    fn code_with_a_language_is_highlighted_while_typing() {
        let draft = Draft::from_lines(&[
            line(LineKind::Code(Some("go".into())), "x // y"),
            line(LineKind::Code(Some("go".into())), "z"),
        ]);
        let style = draft_style(&draft, "Mono".into());
        let ranges: Vec<_> = style.text.iter().map(|d| d.range.clone()).collect();
        // "x " plain, the comment, then "z": all after the 3-byte header mark.
        assert_eq!(ranges[..2], [3..5, 5..9]);
        assert_eq!(
            draft_style(
                &Draft::from_lines(&[line(LineKind::Code(None), "x // y")]),
                "Mono".into()
            )
            .text
            .len(),
            1
        );
    }

    #[test]
    fn the_first_code_line_starts_a_line_for_the_highlighter() {
        let draft = Draft::from_lines(&[line(LineKind::Code(Some("markdown".into())), "# h")]);
        let style = draft_style(&draft, "Mono".into());
        assert_eq!(&draft.text()[style.text[0].range.clone()], "# h");
    }

    #[test]
    fn python_keywords_are_highlighted_in_the_composer() {
        let draft = Draft::from_lines(&[
            line(LineKind::Code(Some("python".into())), "def f():"),
            line(LineKind::Code(Some("python".into())), "class A:"),
        ]);
        let style = draft_style(&draft, "Mono".into());
        let words: Vec<_> = style
            .text
            .iter()
            .map(|d| &draft.text()[d.range.clone()])
            .collect();
        assert!(
            words.contains(&"def") && words.contains(&"class"),
            "{words:?}"
        );
        let mut ranges: Vec<_> = style.text.iter().map(|d| d.range.clone()).collect();
        ranges.sort_by_key(|range| range.start);
        assert!(
            ranges.windows(2).all(|pair| pair[0].end <= pair[1].start),
            "{ranges:?}"
        );
    }

    #[test]
    fn a_lone_code_line_still_gets_a_block() {
        let draft = Draft::from_lines(&[line(LineKind::Code(None), "")]);
        let style = draft_style(&draft, "Mono".into());
        assert_eq!(style.ranges.len(), 1);
        assert_eq!(style.ranges[0].style(), RangeDecorationStyle::Block);
    }
}
