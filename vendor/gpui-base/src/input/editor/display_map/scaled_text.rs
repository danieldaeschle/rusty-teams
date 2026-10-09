use std::ops::Range;

use gpui::{Font, Pixels, TextRun, TextSystem, px};
use smallvec::SmallVec;

use super::text_wrapper::split_run_by_font_overrides;
use crate::input::decorations::FontOverride;

const HEIGHT_EPSILON: Pixels = px(0.01);

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ScalePiece {
    pub(crate) range: Range<usize>,
    pub(crate) scale: f32,
    pub(crate) raise: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FontMetrics {
    pub(crate) ascent: Pixels,
    pub(crate) descent: Pixels,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RowPiece {
    pub(crate) piece: ScalePiece,
    pub(crate) metrics: FontMetrics,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RowBox {
    pub(crate) baseline: Pixels,
    pub(crate) height: Pixels,
    pub(crate) base_baseline: Pixels,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ScaledRow {
    pub(crate) row_box: RowBox,
    pub(crate) pieces: SmallVec<[RowPiece; 4]>,
    pub(crate) font_size: Pixels,
}

impl RowBox {
    pub(crate) fn custom_height(&self, line_height: Pixels) -> Option<Pixels> {
        ((self.height - line_height).abs() > HEIGHT_EPSILON).then_some(self.height)
    }
}

impl ScaledRow {
    pub(crate) fn pieces_in<'a>(
        &'a self,
        range: Range<usize>,
    ) -> impl Iterator<Item = (Range<usize>, &'a RowPiece)> + 'a {
        self.pieces.iter().filter_map(move |row_piece| {
            let start = row_piece.piece.range.start.max(range.start);
            let end = row_piece.piece.range.end.min(range.end);
            (start < end).then_some((start..end, row_piece))
        })
    }

    pub(crate) fn raise_pixels(&self, row_piece: &RowPiece) -> Pixels {
        self.font_size * row_piece.piece.raise
    }
}

pub(crate) fn scale_pieces(
    range: Range<usize>,
    font_overrides: &[(Range<usize>, FontOverride)],
) -> Option<SmallVec<[ScalePiece; 4]>> {
    let first = font_overrides.partition_point(|(span, _)| span.end <= range.start);
    let mut pieces: SmallVec<[ScalePiece; 4]> = SmallVec::new();
    let mut offset = range.start;
    let mut scaled = false;
    let mut push = |range: Range<usize>, scale: f32, raise: f32| match pieces.last_mut() {
        Some(last)
            if last.scale == scale && last.raise == raise && last.range.end == range.start =>
        {
            last.range.end = range.end;
        }
        _ => pieces.push(ScalePiece {
            range,
            scale,
            raise,
        }),
    };
    for (span, font_override) in &font_overrides[first..] {
        if span.start >= range.end {
            break;
        }
        let start = span.start.max(range.start);
        let end = span.end.min(range.end);
        if offset < start {
            push(offset..start, 1., 0.);
        }
        let scale = font_override.scale.unwrap_or(1.);
        let raise = font_override.raise.unwrap_or(0.);
        scaled |= scale != 1. || raise != 0.;
        push(start..end, scale, raise);
        offset = end;
    }
    if offset < range.end {
        push(offset..range.end, 1., 0.);
    }
    scaled.then_some(pieces)
}

fn runs_metrics(text_system: &TextSystem, runs: &[TextRun], font_size: Pixels) -> FontMetrics {
    runs.iter().fold(
        FontMetrics {
            ascent: px(0.),
            descent: px(0.),
        },
        |metrics, run| {
            let font_id = text_system.resolve_font(&run.font);
            FontMetrics {
                ascent: metrics.ascent.max(text_system.ascent(font_id, font_size)),
                descent: metrics.descent.max(text_system.descent(font_id, font_size)),
            }
        },
    )
}

fn centered_baseline(box_height: Pixels, metrics: FontMetrics) -> Pixels {
    (box_height - metrics.ascent - metrics.descent) / 2. + metrics.ascent
}

pub(crate) fn scaled_row(
    text_system: &TextSystem,
    font: &Font,
    font_size: Pixels,
    line_height: Pixels,
    range: Range<usize>,
    font_overrides: &[(Range<usize>, FontOverride)],
) -> Option<ScaledRow> {
    let pieces = scale_pieces(range, font_overrides)?;
    let run = |len: usize| TextRun {
        len,
        font: font.clone(),
        color: gpui::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let base_metrics = runs_metrics(text_system, &[run(0)], font_size);
    let pieces: SmallVec<[RowPiece; 4]> = pieces
        .into_iter()
        .map(|piece| {
            let runs = split_run_by_font_overrides(
                run(piece.range.len()),
                piece.range.clone(),
                font_overrides,
            );
            let metrics = runs_metrics(text_system, &runs, font_size * piece.scale);
            RowPiece { piece, metrics }
        })
        .collect();
    Some(ScaledRow {
        row_box: row_box(line_height, font_size, base_metrics, &pieces),
        pieces,
        font_size,
    })
}

fn row_box(
    line_height: Pixels,
    font_size: Pixels,
    base_metrics: FontMetrics,
    pieces: &[RowPiece],
) -> RowBox {
    let base_baseline = centered_baseline(line_height, base_metrics);
    let mut above = base_baseline;
    let mut below = line_height - base_baseline;
    for row_piece in pieces {
        let height = line_height * row_piece.piece.scale;
        let baseline = centered_baseline(height, row_piece.metrics);
        let raise = font_size * row_piece.piece.raise;
        above = above.max(baseline + raise);
        below = below.max(height - baseline - raise);
    }
    RowBox {
        baseline: above,
        height: above + below,
        base_baseline,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scaled(scale: f32, raise: f32) -> FontOverride {
        FontOverride {
            scale: Some(scale),
            raise: Some(raise),
            ..Default::default()
        }
    }

    #[test]
    fn plain_ranges_have_no_pieces() {
        assert_eq!(scale_pieces(0..10, &[]), None);
        let overrides = [(
            2..4,
            FontOverride {
                family: Some("Mono".into()),
                ..Default::default()
            },
        )];
        assert_eq!(scale_pieces(0..10, &overrides), None);
        assert_eq!(scale_pieces(5..10, &[(0..4, scaled(2., 0.))]), None);
    }

    #[test]
    fn pieces_fill_gaps_clip_to_the_range_and_merge_equal_neighbours() {
        let overrides = [
            (2..4, scaled(2., 0.)),
            (4..6, scaled(2., 0.)),
            (8..12, scaled(0.5, 0.35)),
        ];
        let piece = |range, scale, raise| ScalePiece {
            range,
            scale,
            raise,
        };
        assert_eq!(
            scale_pieces(1..10, &overrides).unwrap().as_slice(),
            [
                piece(1..2, 1., 0.),
                piece(2..6, 2., 0.),
                piece(6..8, 1., 0.),
                piece(8..10, 0.5, 0.35),
            ]
        );
    }

    fn row_piece(scale: f32, raise: f32) -> RowPiece {
        RowPiece {
            piece: ScalePiece {
                range: 0..1,
                scale,
                raise,
            },
            metrics: FontMetrics {
                ascent: px(10.) * scale,
                descent: px(4.) * scale,
            },
        }
    }

    #[test]
    fn row_box_grows_for_large_and_raised_pieces_but_never_shrinks_for_small() {
        let base = FontMetrics {
            ascent: px(10.),
            descent: px(4.),
        };
        let line_height = px(20.);
        let box_for = |scale: f32, raise: f32| {
            row_box(line_height, px(14.), base, &[row_piece(scale, raise)])
        };
        assert_eq!(box_for(1., 0.).custom_height(line_height), None);
        assert_eq!(box_for(9. / 14., 0.).custom_height(line_height), None);
        let large = box_for(24. / 14., 0.);
        assert!((large.height - px(20.) * 24. / 14.).abs() < px(0.01));
        assert!(large.baseline > large.base_baseline);
        assert!(box_for(0.75, 0.35).height > line_height);
        assert!(box_for(0.75, -0.2).height > line_height);
    }

    #[test]
    fn mixed_sizes_share_one_baseline() {
        let base = FontMetrics {
            ascent: px(10.),
            descent: px(4.),
        };
        let mixed = row_box(
            px(20.),
            px(14.),
            base,
            &[row_piece(1., 0.), row_piece(2., 0.), row_piece(0.5, 0.)],
        );
        let large_only = row_box(px(20.), px(14.), base, &[row_piece(2., 0.)]);
        assert_eq!(mixed, large_only);
    }
}
