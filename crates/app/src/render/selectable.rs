use std::ops::Range;

use gpui_kit::base::{
    TextSelection, TextSelectionHandle, TextSelectionRegistration, TextSelectionRun, Theme,
};
use gpui_kit::*;

/// Styled text that takes part in the window text selection, with clickable link ranges.
pub struct SelectableRichText {
    id: ElementId,
    text: SharedString,
    styled_text: StyledText,
    links: Vec<(Range<usize>, String)>,
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
        }
    }

    pub fn links(mut self, links: Vec<(Range<usize>, String)>) -> Self {
        self.links = links;
        self
    }
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
    type RequestLayoutState = TextSelectionHandle;
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
        let handle = window.with_element_state(
            global_id.expect("SelectableRichText has an element id"),
            |retained: Option<TextSelectionHandle>, _| {
                let handle =
                    retained.unwrap_or_else(|| TextSelectionHandle::new(self.text.clone(), cx));
                (handle.clone(), handle)
            },
        );
        let (layout_id, ()) = self
            .styled_text
            .request_layout(global_id, inspector_id, window, cx);
        (layout_id, handle)
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        handle: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.styled_text
            .prepaint(global_id, inspector_id, bounds, &mut (), window, cx);
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
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
        handle: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let layout = self.styled_text.layout().clone();
        let selected_before = TextSelection::selected_text(window, cx);
        let projection = handle.update_runs(
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
        let color = Theme::global(cx).tokens.colors.selection;
        for range in projection.ranges().iter().flatten() {
            let (Some(start), Some(end)) = (
                layout.position_for_index(range.start),
                layout.position_for_index(range.end),
            ) else {
                continue;
            };
            for quad in selection_quads(start, end, layout.bounds(), layout.line_height()) {
                window.paint_quad(fill(quad, color));
            }
        }
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

    use super::SelectableRichText;

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
}
