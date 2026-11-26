use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::*;

use super::hooks::is_openable_link;

const MAX_WIDTH: f32 = 480.;

/// Shows the URL of the link under the mouse in a tooltip, as a browser shows a link's title.
/// Kept in the element's state so the show delay survives redraws.
#[derive(Clone, Default)]
struct LinkTooltipState {
    active: Rc<RefCell<Option<ActiveTooltip>>>,
    long_press: Rc<Cell<bool>>,
    shown_url: Rc<RefCell<Option<String>>>,
    pressed: Rc<Cell<bool>>,
}

impl LinkTooltipState {
    /// A shown tooltip belongs to one link: moving onto another link hides it, so it can show
    /// again with the new URL. No tooltip while a button is held, as during a text selection.
    fn hovers(&self, url: Option<String>) -> bool {
        let Some(url) = url.filter(|_| !self.pressed.get()) else {
            return false;
        };
        let showing = matches!(
            self.active.borrow().as_ref(),
            Some(ActiveTooltip::Visible { .. } | ActiveTooltip::WaitingForHide { .. })
        );
        !showing || self.shown_url.borrow().as_deref() == Some(url.as_str())
    }
}

/// One frame of a text element's link tooltip: made in prepaint, wired up in paint.
pub struct LinkTooltip {
    state: LinkTooltipState,
    id: Option<TooltipId>,
}

impl LinkTooltip {
    pub(super) fn prepaint(
        global_id: Option<&GlobalElementId>,
        has_links: bool,
        window: &mut Window,
    ) -> Option<LinkTooltip> {
        let state =
            window.with_element_state(global_id?, |retained: Option<LinkTooltipState>, _| {
                let state = retained.unwrap_or_default();
                (state.clone(), state)
            });
        if !has_links {
            state.long_press.set(false);
            state.active.take();
            return None;
        }
        let id = set_tooltip_on_window(&state.active, window);
        Some(LinkTooltip { state, id })
    }

    /// `url_at` gives the URL of the link at a window position.
    pub(super) fn paint(
        &self,
        hitbox: &Hitbox,
        url_at: impl Fn(Point<Pixels>) -> Option<String> + 'static,
        window: &mut Window,
    ) {
        let url_at = Rc::new(move |position| url_at(position).filter(|url| is_openable_link(url)));
        let build = Rc::new({
            let (url_at, shown_url) = (url_at.clone(), self.state.shown_url.clone());
            move |window: &mut Window, cx: &mut App| {
                let url = url_at(window.mouse_position())?;
                *shown_url.borrow_mut() = Some(url.clone());
                let tooltip =
                    Tooltip::element(move |_, _| div().max_w(px(MAX_WIDTH)).child(url.clone()));
                Some((tooltip.build(window, cx), false))
            }
        });
        let hovered = Rc::new({
            let (hitbox, url_at, state) = (hitbox.clone(), url_at.clone(), self.state.clone());
            move |window: &Window| {
                hitbox.is_hovered(window) && state.hovers(url_at(window.mouse_position()))
            }
        });
        // Prepaint has no hover state yet, so it tests the bounds instead.
        let hovered_in_prepaint = Rc::new({
            let (bounds, state) = (hitbox.bounds, self.state.clone());
            move |window: &Window| {
                let position = window.mouse_position();
                bounds.contains(&position) && state.hovers(url_at(position))
            }
        });
        let pressed = self.state.pressed.clone();
        let press_hitbox = hitbox.clone();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, _| {
            if phase.bubble()
                && event.button == MouseButton::Left
                && press_hitbox.is_hovered(window)
            {
                pressed.set(true);
            }
        });
        let pressed = self.state.pressed.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, _| {
            if phase.capture() && event.button == MouseButton::Left {
                pressed.set(false);
            }
        });
        register_tooltip_mouse_handlers(
            &self.state.active,
            self.id,
            build,
            hovered,
            hovered_in_prepaint,
            self.state.long_press.clone(),
            None,
            window,
        );
    }
}
