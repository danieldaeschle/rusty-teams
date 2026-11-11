use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::base::{GlobalState, POPUP_PRIORITY};
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::h_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::message_actions::{MenuActions, MessageMenu, picker_button, reaction_button};
use super::toolbar_placement::TOOLBAR_HEIGHT;
use crate::theme::ElevatedSurface as _;

pub const CONTEXT_MENU_TAG: &str = "message-context-menu";
const PANEL_GAP: f32 = 6.;
const BAR_PADDING: f32 = 3.;
const WINDOW_MARGIN: f32 = 8.;
const BAR_LIFT: f32 = TOOLBAR_HEIGHT + PANEL_GAP;
const LIST_OPEN: Duration = Duration::from_millis(160);
const REACTION_OPEN: Duration = Duration::from_millis(140);
const REACTION_STAGGER: Duration = Duration::from_millis(35);
const LIST_SLIDE: f32 = 6.;
const REACTION_RISE: f32 = 8.;

pub type Close = Rc<dyn Fn(&mut App)>;

/// One open right-click menu: the menu entity lives as long as the menu is open, so hover and submenus keep their state.
#[derive(Clone)]
pub struct OpenContextMenu {
    pub key: String,
    pub position: Point<Pixels>,
    pub serial: u64,
    popup: Rc<RefCell<Option<(Entity<PopupMenu>, Subscription)>>>,
    bar_pressed: Rc<Cell<bool>>,
    list_height: Rc<Cell<Option<f32>>>,
}

impl OpenContextMenu {
    pub fn new(key: &str, position: Point<Pixels>, serial: u64) -> Self {
        OpenContextMenu {
            key: key.to_owned(),
            position,
            serial,
            popup: Rc::default(),
            bar_pressed: Rc::default(),
            list_height: Rc::default(),
        }
    }

    fn popup(&self, menu: &MessageMenu, close: Close, window: &mut Window, cx: &mut App) -> Entity<PopupMenu> {
        if let Some((popup, _)) = self.popup.borrow().as_ref() {
            return popup.clone();
        }
        let actions = MenuActions::of(menu);
        let popup = PopupMenu::build(window, cx, move |popup, window, cx| {
            actions.fill(popup, true, window, cx)
        });
        popup.focus_handle(cx).focus(window, cx);
        let bar_pressed = self.bar_pressed.clone();
        let subscription = cx.subscribe(&popup, move |_, _: &DismissEvent, cx| {
            if !bar_pressed.replace(false) {
                close(cx);
            }
        });
        *self.popup.borrow_mut() = Some((popup.clone(), subscription));
        popup
    }
}

fn measure_height(height: Rc<Cell<Option<f32>>>) -> impl IntoElement {
    canvas(
        move |bounds, _, _| height.set(Some(f32::from(bounds.size.height))),
        |_, _, _, _| {},
    )
    .absolute()
    .size_full()
}

fn animated_reaction(button: AnyElement, index: usize, serial: u64) -> AnyElement {
    let delay = REACTION_STAGGER * index as u32;
    let total = delay + REACTION_OPEN;
    let start = delay.as_secs_f32() / total.as_secs_f32();
    div()
        .relative()
        .child(button)
        .with_animation(
            ElementId::Name(format!("context-reaction-{serial}-{index}").into()),
            Animation::new(total).with_easing(ease_out_quint()),
            move |element, delta| {
                let local = ((delta - start) / (1. - start)).clamp(0., 1.);
                element.opacity(local).top(px((1. - local) * REACTION_RISE))
            },
        )
        .into_any_element()
}

/// Telegram's behaviour in our look: reactions above the cursor, the menu below; near the bottom edge it turns upside down.
pub fn message_context_menu(
    mut menu: MessageMenu,
    open: &OpenContextMenu,
    close: Close,
    close_from_outside: Close,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let popup = open.popup(&menu, close_from_outside, window, cx);
    let picked = menu.done.clone();
    let after_pick = close.clone();
    menu.done = Rc::new(move |window, cx| {
        picked(window, cx);
        after_pick(cx);
    });
    let serial = open.serial;
    let mut reactions: Vec<AnyElement> = menu
        .quick
        .iter()
        .enumerate()
        .map(|(index, glyph)| animated_reaction(reaction_button(&menu, glyph, index), index, serial))
        .collect();
    reactions.push(animated_reaction(picker_button(&menu, cx), menu.quick.len(), serial));
    let viewport_height = f32::from(window.viewport_size().height);
    let cursor_y = f32::from(open.position.y);
    let upside_down = open
        .list_height
        .get()
        .is_some_and(|height| cursor_y + height + WINDOW_MARGIN > viewport_height);
    let bar_pressed = open.bar_pressed.clone();
    let bar = h_flex()
        .debug_selector(|| format!("{CONTEXT_MENU_TAG}/reactions").into())
        .absolute()
        .left_0()
        .map(|bar| {
            if upside_down {
                bar.top_full().mt(px(PANEL_GAP))
            } else {
                bar.bottom_full().mb(px(PANEL_GAP))
            }
        })
        .occlude()
        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
        .h(px(TOOLBAR_HEIGHT))
        .px(px(BAR_PADDING))
        .gap(px(1.))
        .items_center()
        .rounded(px(10.))
        .flyout_surface()
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            bar_pressed.set(true);
            GlobalState::suppress_text_selection(cx);
        })
        .children(reactions);
    let slide = if upside_down { LIST_SLIDE } else { -LIST_SLIDE };
    let list = div()
        .relative()
        .debug_selector(|| format!("{CONTEXT_MENU_TAG}/list").into())
        .child(popup)
        .child(measure_height(open.list_height.clone()))
        .with_animation(
            ElementId::Name(format!("context-list-{serial}-{upside_down}").into()),
            Animation::new(LIST_OPEN).with_easing(ease_out_quint()),
            move |element, delta| element.opacity(delta).top(px((1. - delta) * slide)),
        );
    let (anchor_at, corner) = if upside_down {
        let lowest = viewport_height - WINDOW_MARGIN - BAR_LIFT;
        (point(open.position.x, px(cursor_y.min(lowest))), Anchor::BottomLeft)
    } else {
        (
            point(open.position.x, open.position.y.max(px(BAR_LIFT + WINDOW_MARGIN))),
            Anchor::TopLeft,
        )
    };
    deferred(
        anchored().position(anchor_at).anchor(corner).child(
            div()
                .id(ElementId::Name(format!("{CONTEXT_MENU_TAG}-{}", open.key).into()))
                .debug_selector(|| CONTEXT_MENU_TAG.into())
                .relative()
                .occlude()
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .child(list)
                .child(bar),
        ),
    )
    .with_priority(POPUP_PRIORITY)
    .into_any_element()
}
