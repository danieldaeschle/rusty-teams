use std::ops::Range;

use gpui_kit::component::input::{RangeDecoration, RangeDecorationStyle, TextDecoration};
use gpui_kit::*;
use teams_core::{Draft, LineKind, MarkKind};

use crate::theme;

const CODE_PILL_RADIUS: f32 = 4.;
const CODE_BLOCK_RADIUS: f32 = 6.;
const QUOTE_BAR_WIDTH: f32 = 3.;
const QUOTE_INDENT: f32 = 12.;
const CODE_INDENT: f32 = 10.;
const LIST_LEVEL_INDENT: f32 = 18.;

/// How the composer paints a draft: text styles, fills and the list markers to hang under.
pub struct DraftStyle {
    pub text: Vec<TextDecoration>,
    pub ranges: Vec<RangeDecoration>,
    pub markers: Vec<Range<usize>>,
    pub indents: Vec<(usize, Pixels)>,
}

fn line_indent(kind: &LineKind) -> f32 {
    match kind {
        LineKind::Text => 0.,
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
        MarkKind::Code => style.color = Some(theme::text_strong()),
        MarkKind::Link(_) => {
            style.color = Some(theme::accent_text());
            style.underline = Some(underline(Some(theme::accent_text())));
        }
    }
    style
}

/// From the start of the first line of `lines` to the end of the last; Block and Bar cover
/// every line starting inside it, ends included, so empty lines count too.
fn block_range(draft: &Draft, lines: Range<usize>) -> Range<usize> {
    let line_ranges = draft.line_ranges();
    line_ranges[lines.start].start..line_ranges[lines.end - 1].end
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
        let decoration = TextDecoration::new(mark.range.clone(), mark_style(&mark.kind));
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
    let kinds = draft.lines();
    let mut index = 0;
    while index < kinds.len() {
        let kind = &kinds[index];
        let run_end = index
            + kinds[index..]
                .iter()
                .take_while(|other| *other == kind)
                .count();
        for line in index..run_end {
            let marker = draft.marker_range(line);
            let content = draft.content_range(line);
            if !marker.is_empty() {
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
                LineKind::Code(_) => text.push(
                    TextDecoration::new(content, color(theme::text_strong()))
                        .with_font_family(mono.clone()),
                ),
                _ => {}
            }
        }
        let block = match kind {
            LineKind::Code(_) => Some(
                RangeDecoration::new(block_range(draft, index..run_end))
                    .with_style(RangeDecorationStyle::Block)
                    .with_color(theme::code_surface())
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
                (starts[3].start, gpui_kit::px(10.)),
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
    fn a_lone_code_line_still_gets_a_block() {
        let draft = Draft::from_lines(&[line(LineKind::Code(None), "")]);
        let style = draft_style(&draft, "Mono".into());
        assert_eq!(style.ranges.len(), 1);
        assert_eq!(style.ranges[0].style(), RangeDecorationStyle::Block);
    }
}
