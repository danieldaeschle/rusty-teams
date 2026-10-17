use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::*;

pub type HoverChange = Rc<dyn Fn(bool, &mut App)>;
pub type TimelineBounds = Rc<Cell<Option<Bounds<Pixels>>>>;

pub const TOOLBAR_HEIGHT: f32 = 36.;
const TOOLBAR_GAP: f32 = 4.;
const SLOT_HEIGHT: f32 = TOOLBAR_HEIGHT + 2. * TOOLBAR_GAP;

pub fn toolbar_slot(id: ElementId, hover: HoverChange, bar: impl IntoElement) -> Stateful<Div> {
    div()
        .id(id)
        .py(px(TOOLBAR_GAP))
        .block_mouse_except_scroll()
        .on_hover(move |hovered, _, cx| hover(*hovered, cx))
        .child(bar)
}

pub fn record_bounds(timeline: &TimelineBounds, top_inset: Pixels) -> impl IntoElement {
    let timeline = timeline.clone();
    canvas(
        move |bounds, _, _| timeline.set(Some(bounds)),
        |_, _, _, _| {},
    )
    .absolute()
    .top(top_inset)
    .left_0()
    .right_0()
    .bottom_0()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ToolbarSide {
    Above,
    Below,
    ViewportTop,
}

pub fn toolbar_side(anchor: Bounds<Pixels>, viewport: Bounds<Pixels>) -> ToolbarSide {
    let above_top = anchor.top() - px(SLOT_HEIGHT - TOOLBAR_GAP);
    let below_bottom = anchor.bottom() + px(SLOT_HEIGHT - TOOLBAR_GAP);
    if above_top >= viewport.top() {
        ToolbarSide::Above
    } else if below_bottom <= viewport.bottom() {
        ToolbarSide::Below
    } else {
        ToolbarSide::ViewportTop
    }
}

fn toolbar_offset(anchor: Bounds<Pixels>, viewport: Option<Bounds<Pixels>>) -> Pixels {
    let Some(viewport) = viewport else {
        return px(0.);
    };
    match toolbar_side(anchor, viewport) {
        ToolbarSide::Above => px(0.),
        ToolbarSide::Below => anchor.size.height + px(SLOT_HEIGHT),
        ToolbarSide::ViewportTop => viewport.top() - anchor.top() + px(SLOT_HEIGHT),
    }
}

pub fn placed_toolbar(
    toolbar: impl IntoElement,
    timeline: TimelineBounds,
    right_inset: f32,
) -> impl IntoElement {
    PlacedToolbar {
        timeline,
        overlay: div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .child(
                div()
                    .absolute()
                    .top(px(-SLOT_HEIGHT))
                    .right(px(right_inset))
                    .child(deferred(toolbar).with_priority(1)),
            )
            .into_any_element(),
    }
}

struct PlacedToolbar {
    timeline: TimelineBounds,
    overlay: AnyElement,
}

impl Element for PlacedToolbar {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.overlay.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let offset = point(px(0.), toolbar_offset(bounds, self.timeline.get()));
        window.with_element_offset(offset, |window| self.overlay.prepaint(window, cx));
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        _prepaint: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.overlay.paint(window, cx);
    }
}

impl IntoElement for PlacedToolbar {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{SLOT_HEIGHT, TOOLBAR_GAP, ToolbarSide, toolbar_offset, toolbar_side};
    use gpui_kit::{Bounds, point, px, size};

    fn span(top: f32, bottom: f32) -> Bounds<gpui_kit::Pixels> {
        Bounds::new(point(px(0.), px(top)), size(px(400.), px(bottom - top)))
    }

    const VIEWPORT: (f32, f32) = (100., 600.);
    const BAR_ROOM: f32 = SLOT_HEIGHT - TOOLBAR_GAP;

    #[test]
    fn stays_above_with_room_below_the_viewport_top() {
        let viewport = span(VIEWPORT.0, VIEWPORT.1);
        let message = span(VIEWPORT.0 + BAR_ROOM, 300.);
        assert_eq!(toolbar_side(message, viewport), ToolbarSide::Above);
        assert_eq!(toolbar_offset(message, Some(viewport)), px(0.));
    }

    #[test]
    fn flips_below_when_the_bar_would_cross_the_viewport_top() {
        let viewport = span(VIEWPORT.0, VIEWPORT.1);
        let message = span(VIEWPORT.0 + BAR_ROOM - 1., 300.);
        assert_eq!(toolbar_side(message, viewport), ToolbarSide::Below);
        assert_eq!(
            toolbar_offset(message, Some(viewport)),
            message.size.height + px(SLOT_HEIGHT)
        );
    }

    #[test]
    fn sticks_to_the_viewport_top_when_neither_side_fits() {
        let viewport = span(VIEWPORT.0, VIEWPORT.1);
        let message = span(VIEWPORT.0 - 50., VIEWPORT.1 - BAR_ROOM + 1.);
        assert_eq!(toolbar_side(message, viewport), ToolbarSide::ViewportTop);
        let bar_top = message.top() - px(SLOT_HEIGHT) + toolbar_offset(message, Some(viewport));
        assert_eq!(bar_top, viewport.top());
    }

    #[test]
    fn stays_above_without_measured_viewport() {
        assert_eq!(toolbar_offset(span(0., 100.), None), px(0.));
    }
}
