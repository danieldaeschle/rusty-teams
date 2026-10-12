use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::*;

use super::message_actions::HoverChange;

pub type TimelineBounds = Rc<Cell<Option<Bounds<Pixels>>>>;

pub const TOOLBAR_HEIGHT: f32 = 36.;
const TOOLBAR_GAP: f32 = 4.;
const SLOT_HEIGHT: f32 = TOOLBAR_HEIGHT + 2. * TOOLBAR_GAP;

pub fn toolbar_slot(id: ElementId, hover: HoverChange, bar: impl IntoElement) -> Stateful<Div> {
    div()
        .id(id)
        .py(px(TOOLBAR_GAP))
        .occlude()
        .on_hover(move |hovered, _, cx| hover(*hovered, cx))
        .child(bar)
}

pub fn record_bounds(timeline: &TimelineBounds) -> impl IntoElement {
    let timeline = timeline.clone();
    canvas(
        move |bounds, _, _| timeline.set(Some(bounds)),
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
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

impl PlacedToolbar {
    fn flip_offset(&self, anchor: Bounds<Pixels>) -> Pixels {
        let bar_top = anchor.top() - px(SLOT_HEIGHT - TOOLBAR_GAP);
        match self.timeline.get() {
            Some(timeline) if bar_top < timeline.top() => anchor.size.height + px(SLOT_HEIGHT),
            _ => px(0.),
        }
    }
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
        let offset = point(px(0.), self.flip_offset(bounds));
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
