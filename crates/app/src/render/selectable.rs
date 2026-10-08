use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use gpui_kit::base::{
    TextSelection, TextSelectionHandle, TextSelectionRegistration, TextSelectionRun, Theme,
};
use gpui_kit::*;

use super::blocks::CODE_PADDING;

pub(super) const TIME_ROOM: char = '\u{2007}';
pub(super) const HIT_SLOP: f32 = 12.;

#[derive(Clone)]
pub struct Participant {
    pub(super) handle: TextSelectionHandle,
    pub(super) copied: Rc<RefCell<String>>,
}

impl Participant {
    pub(super) fn retained(
        global_id: Option<&GlobalElementId>,
        text: &SharedString,
        window: &mut Window,
        cx: &mut App,
    ) -> Participant {
        window.with_element_state(
            global_id.expect("selectable text has an element id"),
            |retained: Option<Participant>, _| {
                let participant = retained.unwrap_or_else(|| {
                    let handle = TextSelectionHandle::new(text.clone(), cx);
                    let copied = Rc::new(RefCell::new(String::new()));
                    let source = copied.clone();
                    handle.copy_with(move |_| source.borrow().clone(), cx);
                    Participant { handle, copied }
                });
                (participant.clone(), participant)
            },
        )
    }
}

pub(super) const PILL_OUTSET: f32 = 3.;
pub(super) const PILL_INSET_Y: f32 = 1.;

#[derive(Clone, Debug)]
pub struct Pill {
    pub range: Range<usize>,
    pub fill: Hsla,
    pub border: Option<Hsla>,
    pub radius: Pixels,
}

/// Styled text that takes part in the window text selection, with clickable link ranges.
pub struct SelectableRichText {
    id: ElementId,
    text: SharedString,
    styled_text: StyledText,
    links: Vec<(Range<usize>, String)>,
    pills: Vec<Pill>,
}

impl SelectableRichText {
    pub fn new(
        id: impl Into<ElementId>,
        text: impl Into<SharedString>,
        highlights: Vec<(Range<usize>, HighlightStyle)>,
    ) -> Self {
        let text = text.into();
        SelectableRichText {
            id: id.into(),
            styled_text: StyledText::new(text.clone()).with_highlights(highlights),
            text,
            links: Vec::new(),
            pills: Vec::new(),
        }
    }

    pub fn links(mut self, links: Vec<(Range<usize>, String)>) -> Self {
        self.links = links;
        self
    }

    pub fn font_overrides(mut self, overrides: Vec<(Range<usize>, SharedString)>) -> Self {
        if !overrides.is_empty() {
            let styled_text = std::mem::replace(&mut self.styled_text, StyledText::new(""));
            self.styled_text = styled_text.with_font_family_overrides(overrides);
        }
        self
    }

    pub fn pills(mut self, pills: Vec<Pill>) -> Self {
        self.pills = pills;
        self
    }
}

fn paint_pills(pills: &[Pill], text: &str, layout: &TextLayout, window: &mut Window) {
    let line_height = layout.line_height();
    let bounds = layout.bounds();
    for pill in pills {
        let lines = pill_lines(text, pill.range.clone(), bounds.left(), |index| {
            layout.position_for_index(index)
        });
        for line in lines {
            let line = Bounds::from_corners(
                point(line.left - px(PILL_OUTSET), line.top + px(PILL_INSET_Y)),
                point(
                    line.right + px(PILL_OUTSET),
                    line.top + line_height - px(PILL_INSET_Y),
                ),
            );
            let (border_width, border_color) = match pill.border {
                Some(color) => (px(1.), color),
                None => (px(0.), transparent_black()),
            };
            window.paint_quad(quad(
                line,
                pill.radius,
                pill.fill,
                border_width,
                border_color,
                BorderStyle::Solid,
            ));
        }
    }
}

#[derive(Debug, PartialEq)]
struct PillLine {
    left: Pixels,
    right: Pixels,
    top: Pixels,
}

fn pill_lines(
    text: &str,
    range: Range<usize>,
    line_left: Pixels,
    position: impl Fn(usize) -> Option<Point<Pixels>>,
) -> Vec<PillLine> {
    let mut lines: Vec<PillLine> = Vec::new();
    let Some(slice) = text.get(range.clone()) else {
        return lines;
    };
    let mut before = position(range.start);
    for (offset, character) in slice.char_indices() {
        let after = position(range.start + offset + character.len_utf8());
        let (Some(start), Some(end)) = (before, after) else {
            before = after;
            continue;
        };
        before = after;
        if character == '\n' {
            continue;
        }
        // gpui places a wrap-boundary index at the end of the previous line.
        let left = if start.y == end.y { start.x } else { line_left };
        let right = if character.is_whitespace() {
            left
        } else {
            end.x
        };
        match lines.last_mut() {
            Some(line) if line.top == end.y => line.right = right.max(line.right),
            _ => lines.push(PillLine {
                left,
                right,
                top: end.y,
            }),
        }
    }
    lines
}

fn link_at(
    layout: &TextLayout,
    links: &[(Range<usize>, String)],
    position: Point<Pixels>,
) -> Option<String> {
    let index = layout.index_for_position(position).ok()?;
    links
        .iter()
        .find(|(range, _)| range.contains(&index))
        .map(|(_, url)| url.clone())
}

fn selection_quads(
    start: Point<Pixels>,
    end: Point<Pixels>,
    bounds: Bounds<Pixels>,
    line_height: Pixels,
) -> Vec<Bounds<Pixels>> {
    if start.y == end.y {
        return vec![Bounds::from_corners(
            start,
            point(end.x, end.y + line_height),
        )];
    }
    let mut quads = vec![Bounds::from_corners(
        start,
        point(bounds.right(), start.y + line_height),
    )];
    if end.y > start.y + line_height {
        quads.push(Bounds::from_corners(
            point(bounds.left(), start.y + line_height),
            point(bounds.right(), end.y),
        ));
    }
    quads.push(Bounds::from_corners(
        point(bounds.left(), end.y),
        point(end.x, end.y + line_height),
    ));
    quads
}

impl IntoElement for SelectableRichText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for SelectableRichText {
    type RequestLayoutState = Participant;
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let participant = Participant::retained(global_id, &self.text, window, cx);
        let (layout_id, ()) = self
            .styled_text
            .request_layout(global_id, inspector_id, window, cx);
        (layout_id, participant)
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        participant: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let handle = &participant.handle;
        self.styled_text
            .prepaint(global_id, inspector_id, bounds, &mut (), window, cx);
        let slop = Edges {
            left: px(HIT_SLOP),
            right: px(HIT_SLOP),
            ..Default::default()
        };
        let hitbox = window.insert_hitbox(bounds.extend(slop), HitboxBehavior::Normal);
        let registration = TextSelectionRegistration::new(hitbox.clone(), bounds)
            .with_text_bounds(vec![bounds])
            .with_rendered_element(handle, window, cx);
        handle.register(registration, window, cx);
        hitbox
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        participant: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let layout = self.styled_text.layout().clone();
        let selected_before = TextSelection::selected_text(window, cx);
        let projection = participant.handle.update_runs(
            &[TextSelectionRun::new(
                self.text.clone(),
                layout.clone(),
                bounds,
            )],
            cx,
        );
        if selected_before != TextSelection::selected_text(window, cx) {
            window.refresh();
        }
        paint_pills(&self.pills, &self.text, &layout, window);
        let color = Theme::global(cx).tokens.colors.selection;
        let content_end = self.text.trim_end_matches(TIME_ROOM).len();
        let mut copied = String::new();
        for range in projection.ranges().iter().flatten() {
            let end_index = range.end.min(content_end);
            if end_index <= range.start {
                continue;
            }
            copied.push_str(&self.text[range.start..end_index].replace(CODE_PADDING, ""));
            let (Some(start), Some(end)) = (
                layout.position_for_index(range.start),
                layout.position_for_index(end_index),
            ) else {
                continue;
            };
            for quad in selection_quads(start, end, layout.bounds(), layout.line_height()) {
                window.paint_quad(fill(quad, color));
            }
        }
        *participant.copied.borrow_mut() = copied;
        self.styled_text.paint(
            global_id,
            inspector_id,
            bounds,
            &mut (),
            &mut (),
            window,
            cx,
        );

        let hovered_link = hitbox.is_hovered(window)
            && link_at(&layout, &self.links, window.mouse_position()).is_some();
        let cursor = if hovered_link {
            CursorStyle::PointingHand
        } else {
            CursorStyle::IBeam
        };
        window.set_cursor_style(cursor, hitbox);
        // gpui-base updates the selection on drag without notifying a view, so nothing redraws.
        window.on_mouse_event(|event: &MouseMoveEvent, phase, window, _| {
            if phase.bubble() && event.pressed_button == Some(MouseButton::Left) {
                window.refresh();
            }
        });
        if self.links.is_empty() {
            return;
        }
        let links = std::mem::take(&mut self.links);
        let hitbox = hitbox.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if !phase.bubble() || event.button != MouseButton::Left || !hitbox.is_hovered(window) {
                return;
            }
            if !TextSelection::selected_text(window, cx).is_empty() {
                return;
            }
            if let Some(url) = link_at(&layout, &links, event.position) {
                cx.open_url(&url);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::base::TextSelection;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        AnyWindowHandle, AppContext as _, Bounds, Context, FontWeight, HighlightStyle, IntoElement,
        ParentElement as _, Point, Render, Styled as _, TestAppContext, Window, WindowBounds,
        WindowOptions, div, point, px, size,
    };

    use super::{PillLine, SelectableRichText, pill_lines};

    const TEXT: &str = "plain bold link";

    struct Message;

    impl Render for Message {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let bold = HighlightStyle {
                font_weight: Some(FontWeight::BOLD),
                ..Default::default()
            };
            div().w(px(400.)).child(
                SelectableRichText::new("message", TEXT, vec![(6..10, bold)])
                    .links(vec![(11..15, "https://example.com".to_owned())]),
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
    fn drag_selects_styled_text_for_copy(cx: &mut TestAppContext) {
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
    fn click_on_a_link_opens_it_but_plain_text_does_not(cx: &mut TestAppContext) {
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

    #[test]
    fn pill_wrapped_whole_onto_next_line_paints_only_there() {
        let text = "the code";
        let position = |index: usize| {
            Some(if index <= 4 {
                point(px(index as f32 * 10.), px(0.))
            } else {
                point(px((index - 4) as f32 * 10.), px(20.))
            })
        };
        assert_eq!(
            pill_lines(text, 4..8, px(0.), position),
            vec![PillLine {
                left: px(0.),
                right: px(40.),
                top: px(20.),
            }]
        );
    }

    #[test]
    fn pill_split_by_a_wrap_paints_one_piece_per_line_without_the_trailing_space() {
        let text = "ab cd";
        let position = |index: usize| {
            Some(if index <= 3 {
                point(px(index as f32 * 10.), px(0.))
            } else {
                point(px((index - 3) as f32 * 10.), px(20.))
            })
        };
        assert_eq!(
            pill_lines(text, 0..5, px(0.), position),
            vec![
                PillLine {
                    left: px(0.),
                    right: px(20.),
                    top: px(0.),
                },
                PillLine {
                    left: px(0.),
                    right: px(20.),
                    top: px(20.),
                },
            ]
        );
    }
}
