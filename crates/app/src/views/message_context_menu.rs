use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::base::{GlobalState, POPUP_PRIORITY};
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::message_actions::{MenuActions, MessageMenu, picker_button, reaction_button};
use super::toolbar_placement::TOOLBAR_HEIGHT;
use crate::theme::ElevatedSurface as _;

pub const CONTEXT_MENU_TAG: &str = "message-context-menu";
const PANEL_GAP: f32 = 6.;
const BAR_PADDING: f32 = 3.;
const WINDOW_MARGIN: f32 = 8.;
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
}

impl OpenContextMenu {
    pub fn new(key: &str, position: Point<Pixels>, serial: u64) -> Self {
        OpenContextMenu {
            key: key.to_owned(),
            position,
            serial,
            popup: Rc::default(),
            bar_pressed: Rc::default(),
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

/// Telegram's behaviour in our look: quick reactions right above the cursor, the message menu right below it.
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
    let bar_pressed = open.bar_pressed.clone();
    let bar = h_flex()
        .debug_selector(|| format!("{CONTEXT_MENU_TAG}/reactions").into())
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
    let list = div()
        .relative()
        .debug_selector(|| format!("{CONTEXT_MENU_TAG}/list").into())
        .child(popup)
        .with_animation(
            ElementId::Name(format!("context-list-{serial}").into()),
            Animation::new(LIST_OPEN).with_easing(ease_out_quint()),
            |element, delta| element.opacity(delta).top(px((delta - 1.) * LIST_SLIDE)),
        );
    let lifted = open.position - point(px(0.), px(TOOLBAR_HEIGHT + PANEL_GAP));
    deferred(
        anchored()
            .position(lifted)
            .snap_to_window_with_margin(px(WINDOW_MARGIN))
            .child(
                v_flex()
                    .id(ElementId::Name(format!("{CONTEXT_MENU_TAG}-{}", open.key).into()))
                    .debug_selector(|| CONTEXT_MENU_TAG.into())
                    .items_start()
                    .gap(px(PANEL_GAP))
                    .occlude()
                    .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                    .child(bar)
                    .child(list),
            ),
    )
    .with_priority(POPUP_PRIORITY)
    .into_any_element()
}
