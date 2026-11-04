use std::{cell::Cell, rc::Rc, time::Duration};

use gpui_kit::component::button::Button;
use gpui_kit::component::tooltip::{PopupTriggerTooltip as _, TOOLTIP_DEBUG_SELECTOR};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Context, InteractiveElement as _, IntoElement, Modifiers, ParentElement as _,
    Render, StatefulInteractiveElement as _, Styled as _, TestAppContext, VisualTestContext,
    Window, div, point, px,
};

const TOOLTIP_TAG: &str = "probe-tooltip";
const POPUP_TAG: &str = "probe-popup";
const PAST_SHOW_DELAY: Duration = Duration::from_millis(600);

struct Probe;

impl Render for Probe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .debug_selector(|| TOOLTIP_TAG.into())
            .size(px(40.))
            .child("tip")
    }
}

struct Trigger {
    popup_open: bool,
    builds: Rc<Cell<usize>>,
}

impl Render for Trigger {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let builds = self.builds.clone();
        div()
            .size_full()
            .child(
                div()
                    .id("trigger")
                    .size(px(40.))
                    .popup_trigger_tooltip(self.popup_open, move |_, cx| {
                        builds.set(builds.get() + 1);
                        cx.new(|_| Probe).into()
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.popup_open = !this.popup_open;
                        cx.notify();
                    })),
            )
            .when(self.popup_open, |root| {
                root.child(
                    div()
                        .debug_selector(|| POPUP_TAG.into())
                        .mt(px(8.))
                        .size(px(80.))
                        .child("menu"),
                )
            })
    }
}

fn settle(cx: &mut VisualTestContext) {
    cx.executor().advance_clock(PAST_SHOW_DELAY);
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui_kit::test]
fn the_tooltip_hides_on_click_and_stays_hidden_while_its_popup_is_open(cx: &mut TestAppContext) {
    let builds = Rc::new(Cell::new(0));
    let (_, cx) = cx.add_window_view({
        let builds = builds.clone();
        move |_, _| Trigger {
            popup_open: false,
            builds,
        }
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_mouse_move(point(px(10.), px(10.)), None, Modifiers::default());
    settle(cx);
    assert_eq!(builds.get(), 1);
    assert!(
        cx.debug_bounds(TOOLTIP_TAG).is_some(),
        "hover shows the tooltip"
    );

    cx.simulate_click(point(px(10.), px(10.)), Modifiers::default());
    settle(cx);
    assert!(
        cx.debug_bounds(POPUP_TAG).is_some(),
        "the click opens the popup"
    );
    assert!(
        cx.debug_bounds(TOOLTIP_TAG).is_none(),
        "the click hides the tooltip"
    );

    for step in 1..4 {
        let offset = 10. + step as f32;
        cx.simulate_mouse_move(point(px(offset), px(offset)), None, Modifiers::default());
        settle(cx);
    }
    assert_eq!(
        builds.get(),
        1,
        "no tooltip is built while the popup is open"
    );
    assert!(cx.debug_bounds(TOOLTIP_TAG).is_none());

    cx.simulate_click(point(px(10.), px(10.)), Modifiers::default());
    settle(cx);
    cx.simulate_mouse_move(point(px(11.), px(11.)), None, Modifiers::default());
    settle(cx);
    assert!(cx.debug_bounds(POPUP_TAG).is_none());
    assert!(
        cx.debug_bounds(TOOLTIP_TAG).is_some(),
        "after the popup closes the tooltip works again"
    );
}

struct VanishingTrigger {
    visible: bool,
}

impl Render for VanishingTrigger {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().when(self.visible, |root| {
            root.child(
                Button::new("vanishing-trigger")
                    .label("Jump")
                    .tooltip("Jump to newest"),
            )
        })
    }
}

#[gpui_kit::test]
fn a_managed_tooltip_closes_when_its_trigger_is_no_longer_rendered(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let trigger = Rc::new(Cell::new(None));
    let (_, cx) = cx.add_window_view({
        let trigger = trigger.clone();
        move |window, cx| {
            let view = cx.new(|_| VanishingTrigger { visible: true });
            trigger.set(Some(view.clone()));
            gpui_kit::base::Root::new(view, window, cx)
        }
    });
    let trigger = trigger.take().expect("trigger view");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_mouse_move(point(px(10.), px(10.)), None, Modifiers::default());
    settle(cx);
    assert!(
        cx.debug_bounds(TOOLTIP_DEBUG_SELECTOR).is_some(),
        "hover shows the tooltip"
    );

    cx.update(|_, cx| {
        trigger.update(cx, |trigger, cx| {
            trigger.visible = false;
            cx.notify();
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|window, cx| window.simulate_next_frame(cx));
    settle(cx);
    assert!(
        cx.debug_bounds(TOOLTIP_DEBUG_SELECTOR).is_none(),
        "the tooltip closes with its trigger"
    );
}

#[gpui_kit::test]
fn a_managed_tooltip_stays_while_its_trigger_is_rendered(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| VanishingTrigger { visible: true });
        gpui_kit::base::Root::new(view, window, cx)
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_mouse_move(point(px(10.), px(10.)), None, Modifiers::default());
    settle(cx);
    for _ in 0..3 {
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
            window.simulate_next_frame(cx);
        });
        settle(cx);
    }
    assert!(cx.debug_bounds(TOOLTIP_DEBUG_SELECTOR).is_some());
}
