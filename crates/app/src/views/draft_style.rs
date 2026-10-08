use std::ops::Range;

use gpui_kit::component::input::{RangeDecoration, RangeDecorationStyle, TextDecoration};
use gpui_kit::*;
use teams_core::{Draft, LineKind, MarkKind};

use crate::theme;

const CODE_PILL_RADIUS: f32 = 4.;
const CODE_BLOCK_RADIUS: f32 = 6.;
const QUOTE_BAR_WIDTH: f32 = 3.;

/// How the composer paints a draft: text styles, fills and the list markers to hang under.
pub struct DraftStyle {
    pub text: Vec<TextDecoration>,
    pub ranges: Vec<RangeDecoration>,
    pub markers: Vec<Range<usize>>,
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

/// A range the block or bar decoration paints over every line of `lines`; `None` when the
/// block is one empty line it cannot address yet.
fn block_range(draft: &Draft, lines: Range<usize>) -> Option<Range<usize>> {
    let line_ranges = draft.line_ranges();
    let start = line_ranges[lines.start].start;
    let end = line_ranges[lines.end - 1].end;
    if end > start {
        return Some(start..end);
    }
    if end < draft.text().len() {
        return Some(start..start + 1);
    }
    let previous_has_text = lines.start > 0 && !line_ranges[lines.start - 1].is_empty();
    previous_has_text.then(|| start - 1..start)
}

pub fn draft_style(draft: &Draft, mono: SharedString) -> DraftStyle {
    let mut text = Vec::new();
    let mut ranges = Vec::new();
    let mut markers = Vec::new();
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
            LineKind::Code(_) => block_range(draft, index..run_end).map(|range| {
                RangeDecoration::new(range)
                    .with_style(RangeDecorationStyle::Block)
                    .with_color(theme::code_surface())
                    .with_radius(px(CODE_BLOCK_RADIUS))
            }),
            LineKind::Quote => block_range(draft, index..run_end).map(|range| {
                RangeDecoration::new(range)
                    .with_style(RangeDecorationStyle::Bar)
                    .with_color(theme::border_strong())
                    .with_radius(px(QUOTE_BAR_WIDTH))
            }),
            _ => None,
        };
        ranges.extend(block);
        index = run_end;
    }
    DraftStyle {
        text,
        ranges,
        markers,
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::component::input::RangeDecorationStyle;
    use teams_core::{Draft, DraftLine, LineKind};

    use super::{block_range, draft_style};

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
    fn an_empty_code_line_at_the_end_still_gets_its_block() {
        let draft = Draft::from_lines(&[
            DraftLine {
                kind: LineKind::Text,
                number: None,
                content: "intro".into(),
                marks: Vec::new(),
            },
            DraftLine {
                kind: LineKind::Code(None),
                number: None,
                content: String::new(),
                marks: Vec::new(),
            },
        ]);
        assert_eq!(block_range(&draft, 1..2), Some(5..6));
    }
}
