use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use app_base::theme;

pub const EDGE_THICKNESS: f32 = 1.;
const SCROLLED_THRESHOLD: f32 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeRule {
    Always,
    WhenScrolled,
}

pub fn content_hidden_above(offset_y: Pixels) -> bool {
    f32::from(offset_y) < -SCROLLED_THRESHOLD
}

pub fn edge_visible(rule: EdgeRule, offset_y: Pixels) -> bool {
    match rule {
        EdgeRule::Always => true,
        EdgeRule::WhenScrolled => content_hidden_above(offset_y),
    }
}

pub fn edge_divider(tag: &'static str) -> Div {
    div()
        .debug_selector(move || tag.into())
        .w_full()
        .h(px(EDGE_THICKNESS))
        .flex_none()
        .bg(theme::border())
}

pub fn settled_offset(offset: Point<Pixels>, max_offset: Point<Pixels>) -> Point<Pixels> {
    let settle = |value: Pixels, max: Pixels| value.max(-max.max(px(0.))).min(px(0.));
    point(
        settle(offset.x, max_offset.x),
        settle(offset.y, max_offset.y),
    )
}

pub fn scroll_offset(handle: &ScrollHandle) -> Point<Pixels> {
    settled_offset(handle.offset(), handle.max_offset())
}

pub fn pushes_past_edge(offset_y: Pixels, max_offset_y: Pixels, delta: Point<Pixels>) -> bool {
    let vertical = if delta.y.is_zero() { delta.x } else { delta.y };
    if vertical > px(0.) {
        !content_hidden_above(offset_y)
    } else if vertical < px(0.) {
        f32::from(offset_y + max_offset_y) <= SCROLLED_THRESHOLD
    } else {
        false
    }
}

pub fn edge_scroll_guard(handle: &ScrollHandle) -> impl IntoElement {
    let handle = handle.clone();
    canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |_, hitbox, window, _| {
            window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture
                    && hitbox.should_handle_scroll(window)
                    && pushes_past_edge(
                        handle.offset().y,
                        handle.max_offset().y,
                        event.delta.pixel_delta(window.line_height()),
                    )
                {
                    cx.stop_propagation();
                }
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

pub fn with_top_edge(
    scroll_area: impl IntoElement,
    handle: &ScrollHandle,
    tag: &'static str,
) -> Div {
    let visible = edge_visible(EdgeRule::WhenScrolled, scroll_offset(handle).y);
    div()
        .relative()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .w_full()
        .child(edge_scroll_guard(handle))
        .child(scroll_area)
        .when(visible, |area| {
            area.child(edge_divider(tag).absolute().top_0().left_0().right_0())
        })
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use gpui_kit::{
        Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _, Render,
        ScrollDelta, ScrollHandle, ScrollWheelEvent, StatefulInteractiveElement as _, Styled as _,
        TestAppContext, TouchPhase, VisualTestContext, Window, div, point, px,
    };

    use super::{
        EdgeRule, content_hidden_above, edge_visible, pushes_past_edge, scroll_offset,
        settled_offset, with_top_edge,
    };

    const EDGE_TAG: &str = "test-edge-line";
    const ROW_HEIGHT: f32 = 40.;
    const ROW_COUNT: usize = 60;

    struct EdgeList {
        handle: ScrollHandle,
        rendered_offsets: Rc<RefCell<Vec<f32>>>,
    }

    impl Render for EdgeList {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            self.rendered_offsets
                .borrow_mut()
                .push(f32::from(scroll_offset(&self.handle).y));
            let rows =
                (0..ROW_COUNT).map(|row_index| div().id(row_index).h(px(ROW_HEIGHT)).flex_none());
            let scroll_area = div()
                .id("edge-list")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&self.handle)
                .children(rows);
            div().size_full().flex().flex_col().child(with_top_edge(
                scroll_area,
                &self.handle,
                EDGE_TAG,
            ))
        }
    }

    struct Harness<'a> {
        window_cx: &'a mut VisualTestContext,
        handle: ScrollHandle,
        notifications: Rc<Cell<usize>>,
        rendered_offsets: Rc<RefCell<Vec<f32>>>,
    }

    impl Harness<'_> {
        fn draw(&mut self) {
            self.window_cx
                .update(|window, cx| window.draw(cx).clear(cx));
        }

        fn wheel(&mut self, delta_y: f32, touch_phase: TouchPhase) {
            self.window_cx.simulate_event(ScrollWheelEvent {
                position: point(px(100.), px(200.)),
                delta: ScrollDelta::Pixels(point(px(0.), px(delta_y))),
                touch_phase,
                ..Default::default()
            });
            self.draw();
        }

        fn touchpad_swipe(&mut self, delta_y: f32, steps: usize) {
            self.wheel(delta_y, TouchPhase::Started);
            for _ in 0..steps {
                self.wheel(delta_y, TouchPhase::Moved);
            }
            self.wheel(0., TouchPhase::Ended);
        }

        fn offset_y(&self) -> f32 {
            f32::from(self.handle.offset().y)
        }

        fn shows_line(&mut self) -> bool {
            self.window_cx.debug_bounds(EDGE_TAG).is_some()
        }
    }

    fn open(cx: &mut TestAppContext) -> Harness<'_> {
        cx.update(gpui_kit::init);
        let handle = ScrollHandle::new();
        let rendered_offsets = Rc::new(RefCell::new(Vec::new()));
        let (view, window_cx): (Entity<EdgeList>, _) = cx.add_window_view({
            let handle = handle.clone();
            let rendered_offsets = rendered_offsets.clone();
            move |_, _| EdgeList {
                handle,
                rendered_offsets,
            }
        });
        let notifications = Rc::new(Cell::new(0));
        window_cx.update({
            let notifications = notifications.clone();
            move |_, cx| {
                cx.observe(&view, move |_, _| {
                    notifications.set(notifications.get() + 1)
                })
                .detach()
            }
        });
        let mut harness = Harness {
            window_cx,
            handle,
            notifications,
            rendered_offsets,
        };
        harness.draw();
        harness
    }

    #[gpui_kit::test]
    fn touchpad_overscroll_at_the_top_changes_nothing(cx: &mut TestAppContext) {
        let mut harness = open(cx);
        harness.notifications.set(0);
        harness.touchpad_swipe(35., 12);
        assert_eq!(harness.offset_y(), 0.);
        assert_eq!(harness.notifications.get(), 0, "no re-render at the edge");
        assert!(!harness.shows_line());
    }

    #[gpui_kit::test]
    fn touchpad_overscroll_at_the_bottom_changes_nothing(cx: &mut TestAppContext) {
        let mut harness = open(cx);
        harness.wheel(-100_000., TouchPhase::Moved);
        let bottom = harness.offset_y();
        assert!(bottom < -1000.);
        harness.notifications.set(0);
        harness.touchpad_swipe(-35., 12);
        assert_eq!(harness.offset_y(), bottom);
        assert_eq!(harness.notifications.get(), 0);
        assert!(harness.shows_line());
    }

    #[gpui_kit::test]
    fn overshoot_past_the_top_is_never_rendered(cx: &mut TestAppContext) {
        let mut harness = open(cx);
        harness.wheel(-10., TouchPhase::Moved);
        assert_eq!(harness.offset_y(), -10.);
        harness.rendered_offsets.borrow_mut().clear();
        harness.touchpad_swipe(400., 2);
        assert_eq!(harness.offset_y(), 0.);
        let rendered = harness.rendered_offsets.borrow().clone();
        assert!(
            rendered.iter().all(|offset| *offset <= 0.),
            "render saw an offset past the top: {rendered:?}"
        );
        assert!(!harness.shows_line());
    }

    #[gpui_kit::test]
    fn scrolling_down_from_the_top_still_scrolls(cx: &mut TestAppContext) {
        let mut harness = open(cx);
        harness.touchpad_swipe(-30., 4);
        assert_eq!(harness.offset_y(), -150.);
        assert!(harness.shows_line());
        harness.touchpad_swipe(30., 4);
        assert_eq!(harness.offset_y(), 0.);
        assert!(!harness.shows_line());
    }

    #[test]
    fn settled_offset_stays_inside_the_scroll_range() {
        let max = point(px(0.), px(500.));
        assert_eq!(settled_offset(point(px(0.), px(120.)), max).y, px(0.));
        assert_eq!(settled_offset(point(px(0.), px(-900.)), max).y, px(-500.));
        assert_eq!(settled_offset(point(px(0.), px(-200.)), max).y, px(-200.));
        assert_eq!(
            settled_offset(point(px(0.), px(-40.)), point(px(0.), px(0.))).y,
            px(0.)
        );
    }

    #[test]
    fn only_deltas_beyond_an_edge_are_held_back() {
        let max = px(500.);
        assert!(pushes_past_edge(px(0.), max, point(px(0.), px(30.))));
        assert!(pushes_past_edge(px(0.), max, point(px(12.), px(0.))));
        assert!(!pushes_past_edge(px(0.), max, point(px(0.), px(-30.))));
        assert!(!pushes_past_edge(px(-10.), max, point(px(0.), px(30.))));
        assert!(pushes_past_edge(px(-500.), max, point(px(0.), px(-30.))));
        assert!(!pushes_past_edge(px(-500.), max, point(px(0.), px(30.))));
        assert!(!pushes_past_edge(px(0.), max, point(px(0.), px(0.))));
    }

    #[test]
    fn the_list_at_its_top_shows_no_line() {
        assert!(!content_hidden_above(px(0.)));
        assert!(!edge_visible(EdgeRule::WhenScrolled, px(0.)));
    }

    #[test]
    fn subpixel_jitter_at_the_top_shows_no_line() {
        assert!(!edge_visible(EdgeRule::WhenScrolled, px(-0.3)));
    }

    #[test]
    fn a_scrolled_list_shows_the_line() {
        assert!(edge_visible(EdgeRule::WhenScrolled, px(-1.)));
        assert!(edge_visible(EdgeRule::WhenScrolled, px(-480.)));
    }

    #[test]
    fn a_fixed_header_always_shows_the_line() {
        assert!(edge_visible(EdgeRule::Always, px(0.)));
        assert!(edge_visible(EdgeRule::Always, px(-20.)));
    }
}
