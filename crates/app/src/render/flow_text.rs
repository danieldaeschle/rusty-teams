use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use gpui_kit::base::{TextSelection, TextSelectionRegistration, TextSelectionRun, Theme};
use gpui_kit::*;

use super::blocks::CODE_PADDING;
use super::selectable::{HIT_SLOP, PILL_INSET_Y, PILL_OUTSET, Participant, Pill, TIME_ROOM};

const WRAP_TOLERANCE: f32 = 0.5;

#[derive(Clone)]
pub struct FlowSegment {
    pub range: Range<usize>,
    pub highlight: HighlightStyle,
    pub family: Option<SharedString>,
    pub scale: f32,
    pub raise: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Measured {
    width: Pixels,
    baseline: Pixels,
}

#[derive(Debug, Clone, PartialEq)]
struct Piece {
    range: Range<usize>,
    scale: f32,
    raise: f32,
    width: Pixels,
    baseline: Pixels,
    height: Pixels,
}

#[derive(Debug, Clone, PartialEq)]
struct Fragment {
    range: Range<usize>,
    scale: f32,
    origin: Point<Pixels>,
    size: Size<Pixels>,
    line: usize,
}

#[derive(Debug, Clone, PartialEq)]
struct Line {
    top: Pixels,
    height: Pixels,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct FlowLayout {
    fragments: Vec<Fragment>,
    lines: Vec<Line>,
    size: Size<Pixels>,
    wrap_width: Option<Pixels>,
}

#[derive(Clone)]
struct Typography {
    style: TextStyle,
    font_size: Pixels,
    line_height: Pixels,
    rem_size: Pixels,
}

impl Typography {
    fn capture(window: &Window) -> Typography {
        let style = window.text_style();
        let rem_size = window.rem_size();
        Typography {
            font_size: style.font_size.to_pixels(rem_size),
            line_height: style.line_height.to_pixels(style.font_size, rem_size),
            rem_size,
            style,
        }
    }
}

struct Atom {
    word: Range<usize>,
    space: Range<usize>,
}

fn hard_lines(text: &str) -> Vec<Range<usize>> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (newline, _) in text.match_indices('\n') {
        lines.push(start..newline);
        start = newline + 1;
    }
    lines.push(start..text.len());
    lines
}

fn atoms(text: &str, line: Range<usize>) -> Vec<Atom> {
    let mut atoms = Vec::new();
    let mut index = line.start;
    while index < line.end {
        let word_end = text[index..line.end]
            .find(' ')
            .map_or(line.end, |offset| index + offset);
        let space_end = word_end
            + text[word_end..line.end]
                .bytes()
                .take_while(|byte| *byte == b' ')
                .count();
        atoms.push(Atom {
            word: index..word_end,
            space: word_end..space_end,
        });
        index = space_end;
    }
    atoms
}

fn style_spans(segments: &[FlowSegment], range: &Range<usize>) -> Vec<(Range<usize>, f32, f32)> {
    let mut spans: Vec<(Range<usize>, f32, f32)> = Vec::new();
    for segment in segments {
        let start = segment.range.start.max(range.start);
        let end = segment.range.end.min(range.end);
        if start >= end {
            continue;
        }
        match spans.last_mut() {
            Some((last, scale, raise))
                if *scale == segment.scale && *raise == segment.raise && last.end == start =>
            {
                last.end = end;
            }
            _ => spans.push((start..end, segment.scale, segment.raise)),
        }
    }
    spans
}

fn build_lines(
    text: &str,
    segments: &[FlowSegment],
    wrap_width: Option<Pixels>,
    base_line_height: Pixels,
    measure: &mut impl FnMut(&Range<usize>, f32) -> Measured,
) -> Vec<Vec<Piece>> {
    let mut pieces_of = |range: &Range<usize>| -> Vec<Piece> {
        style_spans(segments, range)
            .into_iter()
            .map(|(range, scale, raise)| {
                let measured = measure(&range, scale);
                Piece {
                    range,
                    scale,
                    raise,
                    width: measured.width,
                    baseline: measured.baseline,
                    height: base_line_height * scale,
                }
            })
            .collect()
    };
    let mut lines = Vec::new();
    for hard_line in hard_lines(text) {
        let mut line: Vec<Piece> = Vec::new();
        let mut width = Pixels::ZERO;
        for atom in atoms(text, hard_line) {
            let word = pieces_of(&atom.word);
            let space = pieces_of(&atom.space);
            let word_width: Pixels = word.iter().map(|piece| piece.width).sum();
            let space_width: Pixels = space.iter().map(|piece| piece.width).sum();
            let overflows =
                wrap_width.is_some_and(|limit| width + word_width > limit + px(WRAP_TOLERANCE));
            if overflows && !line.is_empty() {
                lines.push(std::mem::take(&mut line));
                width = Pixels::ZERO;
            }
            line.extend(word);
            line.extend(space);
            width += word_width + space_width;
        }
        lines.push(line);
    }
    lines
}

fn place(
    lines: Vec<Vec<Piece>>,
    base: Measured,
    base_line_height: Pixels,
    base_font_size: Pixels,
    wrap_width: Option<Pixels>,
) -> FlowLayout {
    let mut layout = FlowLayout {
        wrap_width,
        ..FlowLayout::default()
    };
    let mut top = Pixels::ZERO;
    for (line_index, pieces) in lines.into_iter().enumerate() {
        let mut ascent = base.baseline;
        let mut descent = base_line_height - base.baseline;
        for piece in &pieces {
            let raise = base_font_size * piece.raise;
            ascent = ascent.max(piece.baseline + raise);
            descent = descent.max(piece.height - piece.baseline - raise);
        }
        let mut x = Pixels::ZERO;
        let mut fragments: Vec<(Fragment, f32)> = Vec::new();
        for piece in pieces {
            let raise = base_font_size * piece.raise;
            match fragments.last_mut() {
                Some((fragment, last_raise))
                    if fragment.scale == piece.scale
                        && *last_raise == piece.raise
                        && fragment.range.end == piece.range.start =>
                {
                    fragment.range.end = piece.range.end;
                    fragment.size.width += piece.width;
                }
                _ => fragments.push((
                    Fragment {
                        range: piece.range.clone(),
                        scale: piece.scale,
                        origin: point(x, top + ascent - piece.baseline - raise),
                        size: size(piece.width, piece.height),
                        line: line_index,
                    },
                    piece.raise,
                )),
            }
            x += piece.width;
        }
        layout.size.width = layout.size.width.max(x);
        layout.lines.push(Line {
            top,
            height: ascent + descent,
        });
        layout
            .fragments
            .extend(fragments.into_iter().map(|(fragment, _)| fragment));
        top += ascent + descent;
    }
    layout.size.height = top;
    if let Some(limit) = wrap_width {
        layout.size.width = layout.size.width.min(limit);
    }
    layout
}

fn text_runs(style: &TextStyle, segments: &[FlowSegment], range: &Range<usize>) -> Vec<TextRun> {
    let mut runs = Vec::new();
    let mut covered = range.start;
    for segment in segments {
        let start = segment.range.start.max(range.start);
        let end = segment.range.end.min(range.end);
        if start >= end {
            continue;
        }
        if covered < start {
            runs.push(style.clone().to_run(start - covered));
        }
        let mut run = style
            .clone()
            .highlight(segment.highlight)
            .to_run(end - start);
        if let Some(family) = &segment.family {
            run.font.family = family.clone();
        }
        runs.push(run);
        covered = end;
    }
    if covered < range.end {
        runs.push(style.clone().to_run(range.end - covered));
    }
    runs
}

fn compute_layout(
    text: &str,
    segments: &[FlowSegment],
    typography: &Typography,
    wrap_width: Option<Pixels>,
    window: &mut Window,
) -> FlowLayout {
    let mut measure = |range: &Range<usize>, scale: f32| {
        let runs = text_runs(&typography.style, segments, range);
        let line = window.text_system().shape_line(
            SharedString::from(text[range.clone()].to_owned()),
            typography.font_size * scale,
            &runs,
            None,
        );
        let height = typography.line_height * scale;
        Measured {
            width: line.width(),
            baseline: (height - line.ascent - line.descent) / 2. + line.ascent,
        }
    };
    let strut = {
        let runs = vec![typography.style.clone().to_run(1)];
        let line = window
            .text_system()
            .shape_line(" ".into(), typography.font_size, &runs, None);
        Measured {
            width: Pixels::ZERO,
            baseline: (typography.line_height - line.ascent - line.descent) / 2. + line.ascent,
        }
    };
    let lines = build_lines(
        text,
        segments,
        wrap_width,
        typography.line_height,
        &mut measure,
    );
    place(
        lines,
        strut,
        typography.line_height,
        typography.font_size,
        wrap_width,
    )
}

pub struct FlowText {
    id: ElementId,
    text: SharedString,
    segments: Vec<FlowSegment>,
    links: Vec<(Range<usize>, String)>,
    pills: Vec<Pill>,
}

impl FlowText {
    pub fn new(
        id: impl Into<ElementId>,
        text: impl Into<SharedString>,
        segments: Vec<FlowSegment>,
    ) -> Self {
        FlowText {
            id: id.into(),
            text: text.into(),
            segments,
            links: Vec::new(),
            pills: Vec::new(),
        }
    }

    pub fn links(mut self, links: Vec<(Range<usize>, String)>) -> Self {
        self.links = links;
        self
    }

    pub fn pills(mut self, pills: Vec<Pill>) -> Self {
        self.pills = pills;
        self
    }
}

pub struct FlowRequest {
    participant: Participant,
    typography: Typography,
    measured: Rc<RefCell<Option<Rc<FlowLayout>>>>,
}

struct PaintedFragment {
    element: AnyElement,
    layout: TextLayout,
    fragment: Fragment,
    bounds: Bounds<Pixels>,
    band: Bounds<Pixels>,
}

pub struct FlowPrepaint {
    hitbox: Hitbox,
    fragments: Vec<PaintedFragment>,
    lines: Vec<Line>,
}

impl IntoElement for FlowText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl FlowText {
    fn fragment_text(&self, fragment: &Fragment) -> SharedString {
        SharedString::from(self.text[fragment.range.clone()].to_owned())
    }

    fn fragment_styled_text(&self, fragment: &Fragment) -> StyledText {
        let mut highlights = Vec::new();
        let mut families = Vec::new();
        for segment in &self.segments {
            let start = segment.range.start.max(fragment.range.start);
            let end = segment.range.end.min(fragment.range.end);
            if start >= end {
                continue;
            }
            let local = start - fragment.range.start..end - fragment.range.start;
            if segment.highlight != HighlightStyle::default() {
                highlights.push((local.clone(), segment.highlight));
            }
            if let Some(family) = &segment.family {
                families.push((local, family.clone()));
            }
        }
        let styled = StyledText::new(self.fragment_text(fragment)).with_highlights(highlights);
        if families.is_empty() {
            styled
        } else {
            styled.with_font_family_overrides(families)
        }
    }

    fn lay_out_fragment(
        &self,
        fragment: &Fragment,
        area: Bounds<Pixels>,
        typography: &Typography,
        window: &mut Window,
        cx: &mut App,
    ) -> (AnyElement, TextLayout) {
        let styled = self.fragment_styled_text(fragment);
        let text_layout = styled.layout().clone();
        let mut element = styled.into_any_element();
        let mut style = typography.style.clone();
        style.font_size = (typography.font_size * fragment.scale).into();
        style.line_height = area.size.height.into();
        style.white_space = WhiteSpace::Nowrap;
        window.with_rem_size(Some(typography.rem_size), |window| {
            window.with_text_style(Some(style.subtract(&Default::default())), |window| {
                element.prepaint_as_root(
                    area.origin,
                    size(
                        AvailableSpace::Definite(area.size.width),
                        AvailableSpace::Definite(area.size.height),
                    ),
                    window,
                    cx,
                );
            });
        });
        (element, text_layout)
    }

    fn link_at(
        prepaint: &[PaintedFragment],
        links: &[(Range<usize>, String)],
        position: Point<Pixels>,
    ) -> Option<String> {
        prepaint.iter().find_map(|painted| {
            let index = painted.layout.index_for_position(position).ok()?;
            let global = painted.fragment.range.start + index;
            links
                .iter()
                .find(|(range, _)| range.contains(&global))
                .map(|(_, url)| url.clone())
        })
    }

    fn paint_pills(&self, prepaint: &[PaintedFragment], window: &mut Window) {
        for pill in &self.pills {
            for painted in prepaint {
                let start = pill.range.start.max(painted.fragment.range.start);
                let end = pill.range.end.min(painted.fragment.range.end);
                if start >= end {
                    continue;
                }
                let offset = painted.fragment.range.start;
                let (Some(from), Some(to)) = (
                    painted.layout.position_for_index(start - offset),
                    painted.layout.position_for_index(end - offset),
                ) else {
                    continue;
                };
                let left = if start == pill.range.start {
                    from.x - px(PILL_OUTSET)
                } else {
                    from.x
                };
                let right = if end == pill.range.end {
                    to.x + px(PILL_OUTSET)
                } else {
                    to.x
                };
                let area = Bounds::from_corners(
                    point(left, painted.bounds.top() + px(PILL_INSET_Y)),
                    point(right, painted.bounds.bottom() - px(PILL_INSET_Y)),
                );
                let (border_width, border_color) = match pill.border {
                    Some(color) => (px(1.), color),
                    None => (px(0.), transparent_black()),
                };
                window.paint_quad(quad(
                    area,
                    pill.radius,
                    pill.fill,
                    border_width,
                    border_color,
                    BorderStyle::Solid,
                ));
            }
        }
    }
}

impl Element for FlowText {
    type RequestLayoutState = FlowRequest;
    type PrepaintState = FlowPrepaint;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let participant = Participant::retained(global_id, &self.text, window, cx);
        let typography = Typography::capture(window);
        let measured: Rc<RefCell<Option<Rc<FlowLayout>>>> = Rc::default();
        let text = self.text.clone();
        let segments = self.segments.clone();
        let layout_id = window.request_measured_layout(Default::default(), {
            let typography = typography.clone();
            let measured = measured.clone();
            move |known_dimensions, available_space, window, _| {
                let wrap_width = known_dimensions.width.or(match available_space.width {
                    AvailableSpace::Definite(width) => Some(width),
                    _ => None,
                });
                let layout = compute_layout(&text, &segments, &typography, wrap_width, window);
                let size = layout.size;
                *measured.borrow_mut() = Some(Rc::new(layout));
                size
            }
        });
        (
            layout_id,
            FlowRequest {
                participant,
                typography,
                measured,
            },
        )
    }

    fn prepaint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let typography = request.typography.clone();
        let measured = request.measured.borrow().clone();
        let layout = match measured {
            Some(layout)
                if layout.wrap_width.is_some_and(|width| {
                    (width - bounds.size.width).abs() < px(WRAP_TOLERANCE)
                }) =>
            {
                layout
            }
            _ => Rc::new(compute_layout(
                &self.text,
                &self.segments,
                &typography,
                Some(bounds.size.width),
                window,
            )),
        };
        let mut fragments = Vec::with_capacity(layout.fragments.len());
        for fragment in &layout.fragments {
            let band = Bounds::new(
                point(
                    bounds.origin.x + fragment.origin.x,
                    bounds.origin.y + layout.lines[fragment.line].top,
                ),
                size(fragment.size.width, layout.lines[fragment.line].height),
            );
            let origin = bounds.origin + fragment.origin;
            let glyphs = Bounds::new(origin, fragment.size);
            let (element, _) = self.lay_out_fragment(fragment, glyphs, &typography, window, cx);
            let (_, layout) = self.lay_out_fragment(fragment, band, &typography, window, cx);
            fragments.push(PaintedFragment {
                element,
                layout,
                fragment: fragment.clone(),
                bounds: glyphs,
                band,
            });
        }
        let slop = Edges {
            left: px(HIT_SLOP),
            right: px(HIT_SLOP),
            ..Default::default()
        };
        let hitbox = window.insert_hitbox(bounds.extend(slop), HitboxBehavior::Normal);
        let registration = TextSelectionRegistration::new(hitbox.clone(), bounds)
            .with_text_bounds(fragments.iter().map(|painted| painted.band).collect())
            .with_rendered_element(&request.participant.handle, window, cx);
        request
            .participant
            .handle
            .register(registration, window, cx);
        FlowPrepaint {
            hitbox,
            fragments,
            lines: layout.lines.clone(),
        }
    }

    fn paint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let participant = &request.participant;
        let selected_before = TextSelection::selected_text(window, cx);
        let runs: Vec<TextSelectionRun> = prepaint
            .fragments
            .iter()
            .enumerate()
            .map(|(order, painted)| {
                TextSelectionRun::new(
                    self.fragment_text(&painted.fragment),
                    painted.layout.clone(),
                    painted.band,
                )
                .with_document_order(order as u64)
            })
            .collect();
        let projection = participant.handle.update_runs(&runs, cx);
        if selected_before != TextSelection::selected_text(window, cx) {
            window.refresh();
        }
        self.paint_pills(&prepaint.fragments, window);

        let color = Theme::global(cx).tokens.colors.selection;
        let content_end = self.text.trim_end_matches(TIME_ROOM).len();
        let ranges = projection.ranges();
        let mut copied = String::new();
        let mut previous_end: Option<usize> = None;
        for (index, painted) in prepaint.fragments.iter().enumerate() {
            let Some(Some(range)) = ranges.get(index) else {
                continue;
            };
            let offset = painted.fragment.range.start;
            let end_index = (offset + range.end).min(content_end);
            let start_index = offset + range.start;
            if end_index <= start_index {
                continue;
            }
            if let Some(end) = previous_end
                && self.text[end..start_index].contains('\n')
            {
                copied.push('\n');
            }
            previous_end = Some(end_index);
            copied.push_str(&self.text[start_index..end_index].replace(CODE_PADDING, ""));
            let (Some(from), Some(to)) = (
                painted.layout.position_for_index(range.start),
                painted.layout.position_for_index(end_index - offset),
            ) else {
                continue;
            };
            let line = &prepaint.lines[painted.fragment.line];
            let top = bounds.top() + line.top;
            let reaches_line_end = end_index >= painted.fragment.range.end
                && prepaint
                    .fragments
                    .get(index + 1)
                    .is_some_and(|next| next.fragment.line != painted.fragment.line)
                && ranges.iter().skip(index + 1).any(Option::is_some);
            let right = if reaches_line_end {
                bounds.right()
            } else {
                to.x
            };
            window.paint_quad(fill(
                Bounds::from_corners(point(from.x, top), point(right, top + line.height)),
                color,
            ));
        }
        *participant.copied.borrow_mut() = copied;

        for painted in prepaint.fragments.iter_mut() {
            painted.element.paint(window, cx);
        }

        let hovered_link = prepaint.hitbox.is_hovered(window)
            && Self::link_at(&prepaint.fragments, &self.links, window.mouse_position()).is_some();
        let cursor = if hovered_link {
            CursorStyle::PointingHand
        } else {
            CursorStyle::IBeam
        };
        window.set_cursor_style(cursor, &prepaint.hitbox);
        window.on_mouse_event(|event: &MouseMoveEvent, phase, window, _| {
            if phase.bubble() && event.pressed_button == Some(MouseButton::Left) {
                window.refresh();
            }
        });
        if self.links.is_empty() {
            return;
        }
        let links = std::mem::take(&mut self.links);
        let hitbox = prepaint.hitbox.clone();
        let layouts: Vec<(TextLayout, usize)> = prepaint
            .fragments
            .iter()
            .map(|painted| (painted.layout.clone(), painted.fragment.range.start))
            .collect();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if !phase.bubble() || event.button != MouseButton::Left || !hitbox.is_hovered(window) {
                return;
            }
            if !TextSelection::selected_text(window, cx).is_empty() {
                return;
            }
            let url = layouts.iter().find_map(|(layout, offset)| {
                let index = layout.index_for_position(event.position).ok()?;
                links
                    .iter()
                    .find(|(range, _)| range.contains(&(offset + index)))
                    .map(|(_, url)| url.clone())
            });
            if let Some(url) = url {
                cx.open_url(&url);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Range;

    use gpui_kit::base::TextSelection;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        AnyWindowHandle, AppContext as _, Bounds, Context, HighlightStyle, IntoElement,
        ParentElement as _, Pixels, Point, Render, Styled as _, TestAppContext, Window,
        WindowBounds, WindowOptions, div, point, px, size,
    };

    use super::{FlowLayout, FlowSegment, FlowText, Measured, build_lines, place};

    const BASE_HEIGHT: f32 = 20.;

    fn segment(range: Range<usize>, scale: f32, raise: f32) -> FlowSegment {
        FlowSegment {
            range,
            highlight: HighlightStyle::default(),
            family: None,
            scale,
            raise,
        }
    }

    fn measure(range: &Range<usize>, scale: f32) -> Measured {
        Measured {
            width: px(range.len() as f32 * 10. * scale),
            baseline: px(BASE_HEIGHT * scale * 0.8),
        }
    }

    fn layout(text: &str, segments: &[FlowSegment], wrap: Option<f32>) -> FlowLayout {
        let wrap_width = wrap.map(px);
        let lines = build_lines(text, segments, wrap_width, px(BASE_HEIGHT), &mut measure);
        place(
            lines,
            Measured {
                width: Pixels::ZERO,
                baseline: px(BASE_HEIGHT * 0.8),
            },
            px(BASE_HEIGHT),
            px(14.),
            wrap_width,
        )
    }

    #[test]
    fn words_wrap_at_the_width_and_keep_their_trailing_space_on_the_line() {
        let text = "aaa bbb ccc";
        let flow = layout(text, &[segment(0..11, 1., 0.)], Some(75.));
        let ranges: Vec<_> = flow.fragments.iter().map(|f| f.range.clone()).collect();
        assert_eq!(ranges, vec![0..8, 8..11]);
        assert_eq!(flow.fragments[1].origin.y, px(BASE_HEIGHT));
        assert_eq!(flow.size.height, px(2. * BASE_HEIGHT));
    }

    #[test]
    fn a_word_is_never_split_across_scale_changes_when_wrapping() {
        let text = "aa H2O";
        let segments = [
            segment(0..4, 1., 0.),
            segment(4..5, 0.75, -0.2),
            segment(5..6, 1., 0.),
        ];
        let flow = layout(text, &segments, Some(45.));
        let lines: Vec<_> = flow.fragments.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![0, 1, 1, 1]);
    }

    #[test]
    fn a_subscript_hangs_below_the_baseline_and_a_superscript_rises_above_it() {
        let text = "x2";
        let sub = layout(
            text,
            &[segment(0..1, 1., 0.), segment(1..2, 0.75, -0.2)],
            None,
        );
        let sup = layout(
            text,
            &[segment(0..1, 1., 0.), segment(1..2, 0.75, 0.35)],
            None,
        );
        let baseline = |flow: &FlowLayout, index: usize, scale: f32| {
            flow.fragments[index].origin.y + px(BASE_HEIGHT * scale * 0.8)
        };
        assert!(baseline(&sub, 1, 0.75) > baseline(&sub, 0, 1.));
        assert!(baseline(&sup, 1, 0.75) < baseline(&sup, 0, 1.));
        assert!(sup.size.height >= px(BASE_HEIGHT));
    }

    #[test]
    fn large_text_makes_its_line_taller_and_small_text_does_not_shrink_it() {
        let text = "ab";
        let large = layout(text, &[segment(0..1, 1., 0.), segment(1..2, 1.5, 0.)], None);
        let small = layout(
            text,
            &[segment(0..1, 1., 0.), segment(1..2, 0.75, 0.)],
            None,
        );
        assert_eq!(large.size.height, px(BASE_HEIGHT * 1.5));
        assert_eq!(small.size.height, px(BASE_HEIGHT));
    }

    #[test]
    fn a_newline_starts_a_line_and_belongs_to_no_fragment() {
        let flow = layout("ab\ncd", &[segment(0..5, 1., 0.)], None);
        let ranges: Vec<_> = flow.fragments.iter().map(|f| f.range.clone()).collect();
        assert_eq!(ranges, vec![0..2, 3..5]);
        assert_eq!(flow.lines.len(), 2);
    }

    struct Message;

    const TEXT: &str = "H2O big link end";

    impl Render for Message {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w(px(400.)).text_size(px(14.)).child(
                FlowText::new(
                    "flow",
                    TEXT,
                    vec![
                        segment(0..1, 1., 0.),
                        segment(1..2, 0.75, -0.2),
                        segment(2..4, 1., 0.),
                        segment(4..7, 1.5, 0.),
                        segment(7..16, 1., 0.),
                    ],
                )
                .links(vec![(8..12, "https://example.com".to_owned())]),
            )
        }
    }

    fn mount(cx: &mut TestAppContext) -> AnyWindowHandle {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Point::default(),
                    size: size(px(400.), px(100.)),
                })),
                ..Default::default()
            };
            gpui_kit::open_window(options, cx, |_, cx| cx.new(|_| Message))
                .expect("open test window")
                .0
        })
    }

    #[gpui_kit::test]
    fn dragging_across_sized_fragments_selects_the_whole_text(cx: &mut TestAppContext) {
        let handle = mount(cx);
        let selected = cx
            .update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                window.drag(point(px(1.), px(8.)), point(px(399.), px(8.)), cx);
                window.render_frame(cx);
                TextSelection::selected_text(window, cx)
            })
            .unwrap();
        assert_eq!(selected, TEXT);
        assert_eq!(cx.opened_url(), None);
    }

    #[gpui_kit::test]
    fn a_link_after_sized_text_opens_and_plain_text_does_not(cx: &mut TestAppContext) {
        let handle = mount(cx);
        let click = |x: f32, cx: &mut TestAppContext| {
            cx.update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                window.drag(point(px(x), px(8.)), point(px(x), px(8.)), cx);
            })
            .unwrap();
        };
        click(2., cx);
        assert_eq!(cx.opened_url(), None);
        let link_x = (40..400).map(|x| x as f32).find(|x| {
            click(*x, cx);
            cx.opened_url().is_some()
        });
        assert!(link_x.is_some());
        assert_eq!(cx.opened_url().as_deref(), Some("https://example.com"));
    }
}
