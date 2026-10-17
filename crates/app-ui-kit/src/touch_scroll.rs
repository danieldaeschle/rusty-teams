use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::{
    Context, InteractiveElement as _, IntoElement, ListAlignment, ListState, ParentElement as _,
    Render, ScrollHandle, StatefulInteractiveElement as _, Styled as _, TestAppContext, TouchEvent,
    TouchId, TouchPhase, VisualTestContext, Window, div, list, point, px,
};

const ROW_HEIGHT: f32 = 40.;
const ROW_COUNT: usize = 60;

struct ScrollContainer {
    scroll_handle: ScrollHandle,
    clicks: Rc<Cell<usize>>,
}

impl Render for ScrollContainer {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let clicks = self.clicks.clone();
        div()
            .id("scroll-container")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll_handle)
            .children((0..ROW_COUNT).map(|row_index| {
                let clicks = clicks.clone();
                div()
                    .id(row_index)
                    .h(px(ROW_HEIGHT))
                    .w_full()
                    .on_click(move |_, _, _| clicks.set(clicks.get() + 1))
            }))
    }
}

struct VirtualList {
    list_state: ListState,
}

impl Render for VirtualList {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        list(self.list_state.clone(), |row_index, _, _| {
            div()
                .id(row_index)
                .h(px(ROW_HEIGHT))
                .w_full()
                .into_any_element()
        })
        .size_full()
    }
}

struct Finger<'a> {
    window_cx: &'a mut VisualTestContext,
    touch_id: TouchId,
}

impl Finger<'_> {
    fn touch(&mut self, phase: TouchPhase, x: f32, y: f32) {
        self.window_cx.simulate_event(TouchEvent {
            id: self.touch_id,
            phase,
            position: point(px(x), px(y)),
            predicted_position: None,
            force: None,
        });
        self.window_cx
            .executor()
            .advance_clock(Duration::from_millis(8));
    }

    fn pan(&mut self, from_y: f32, to_y: f32, steps: usize) {
        self.touch(TouchPhase::Started, 100., from_y);
        for step in 1..=steps {
            let progress = step as f32 / steps as f32;
            self.touch(TouchPhase::Moved, 100., from_y + (to_y - from_y) * progress);
        }
        self.touch(TouchPhase::Ended, 100., to_y);
    }

    fn tap(&mut self, x: f32, y: f32) {
        self.touch(TouchPhase::Started, x, y);
        self.touch(TouchPhase::Ended, x, y);
        self.window_cx
            .executor()
            .advance_clock(Duration::from_millis(500));
    }
}

fn open_scroll_container(
    cx: &mut TestAppContext,
) -> (ScrollHandle, Rc<Cell<usize>>, &mut VisualTestContext) {
    cx.update(gpui_kit::init);
    let scroll_handle = ScrollHandle::new();
    let clicks = Rc::new(Cell::new(0));
    let (_, window_cx) = cx.add_window_view({
        let scroll_handle = scroll_handle.clone();
        let clicks = clicks.clone();
        move |_, _| ScrollContainer {
            scroll_handle,
            clicks,
        }
    });
    window_cx.update(|window, cx| window.draw(cx).clear(cx));
    (scroll_handle, clicks, window_cx)
}

fn run_frames(window_cx: &mut VisualTestContext, frame_count: usize) {
    for _ in 0..frame_count {
        window_cx
            .executor()
            .advance_clock(Duration::from_millis(16));
        window_cx.update(|window, cx| {
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        });
    }
}

#[gpui_kit::test]
fn finger_pan_scrolls_the_container_under_the_finger_by_pixels(cx: &mut TestAppContext) {
    let (scroll_handle, clicks, window_cx) = open_scroll_container(cx);
    let mut finger = Finger {
        window_cx,
        touch_id: TouchId(1),
    };
    finger.pan(300., 180., 30);
    let offset_y = f32::from(scroll_handle.offset().y);
    assert!(
        (-120.5..=-100.).contains(&offset_y),
        "a 120 px slow pan scrolls about 120 px, got {offset_y}"
    );
    assert_eq!(clicks.get(), 0, "a pan never clicks");
}

#[gpui_kit::test]
fn fling_keeps_scrolling_after_release(cx: &mut TestAppContext) {
    let (scroll_handle, _, window_cx) = open_scroll_container(cx);
    let mut finger = Finger {
        window_cx,
        touch_id: TouchId(1),
    };
    finger.pan(300., 100., 5);
    let released_at = f32::from(scroll_handle.offset().y);
    run_frames(finger.window_cx, 30);
    let coasted_to = f32::from(scroll_handle.offset().y);
    assert!(
        coasted_to < released_at - 50.,
        "momentum continues the scroll after release: {released_at} -> {coasted_to}"
    );
}

#[gpui_kit::test]
fn tap_clicks_the_row_under_the_finger(cx: &mut TestAppContext) {
    let (_, clicks, window_cx) = open_scroll_container(cx);
    let mut finger = Finger {
        window_cx,
        touch_id: TouchId(1),
    };
    finger.tap(100., 60.);
    assert_eq!(clicks.get(), 1);
}

#[gpui_kit::test]
fn finger_pan_scrolls_a_virtual_list(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let list_state = ListState::new(ROW_COUNT, ListAlignment::Top, px(200.));
    let (_, window_cx) = cx.add_window_view({
        let list_state = list_state.clone();
        move |_, _| VirtualList { list_state }
    });
    window_cx.update(|window, cx| window.draw(cx).clear(cx));
    let mut finger = Finger {
        window_cx,
        touch_id: TouchId(1),
    };
    finger.pan(300., 100., 30);
    let scroll_top = list_state.logical_scroll_top();
    let scrolled = scroll_top.item_ix as f32 * ROW_HEIGHT + f32::from(scroll_top.offset_in_item);
    assert!(
        (180.0..=200.5).contains(&scrolled),
        "a 200 px slow pan scrolls the list about 200 px, got {scrolled}"
    );
}
