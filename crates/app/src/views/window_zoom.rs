use gpui_kit::component::{h_flex, tooltip::Tooltip};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::widgets::symbol;
use crate::active_appearance;
use crate::app_state::AppState;
use crate::appearance::{self, DEFAULT_ZOOM};
use crate::theme;

const ZOOM_LABEL: &str = "Zoom";
const ZOOM_IN_LABEL: &str = "Zoom in";
const ZOOM_OUT_LABEL: &str = "Zoom out";
const ZOOM_RESET_LABEL: &str = "Reset zoom";
const DEFAULT_PERCENT: u32 = 100;
const MINIMUM_TOAST_PERCENT: u32 = 60;
const MAXIMUM_TOAST_PERCENT: u32 = 400;
const MENU_ROUNDING: u32 = 5;
const BUTTON_SIZE: f32 = 28.;
const BUTTON_ICON: f32 = 18.;
const BUTTON_RADIUS: f32 = 6.;
const DISABLED_OPACITY: f32 = 0.4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowZoom {
    In,
    Out,
    Reset,
}

impl WindowZoom {
    fn next(self, zoom: f32) -> f32 {
        match self {
            WindowZoom::In => appearance::zoom_in(zoom),
            WindowZoom::Out => appearance::zoom_out(zoom),
            WindowZoom::Reset => DEFAULT_ZOOM,
        }
    }
}

pub fn toast_text(previous_percent: u32, percent: u32) -> String {
    let state = if percent == DEFAULT_PERCENT {
        "Default size"
    } else if percent <= MINIMUM_TOAST_PERCENT {
        "Minimum zoom"
    } else if percent >= MAXIMUM_TOAST_PERCENT {
        "Maximum zoom"
    } else if percent > previous_percent {
        "Zoomed in"
    } else {
        "Zoomed out"
    };
    format!("{percent}% \u{b7} {state}")
}

pub fn menu_level(zoom: f32) -> String {
    let percent = appearance::zoom_percent(zoom);
    let rounded = (percent + MENU_ROUNDING / 2) / MENU_ROUNDING * MENU_ROUNDING;
    format!("({rounded})%")
}

pub fn change_zoom(app: &Entity<AppState>, change: WindowZoom, cx: &mut App) {
    let previous = active_appearance::current(cx).zoom;
    let next = change.next(previous);
    let store = app.read(cx).store.clone();
    if !active_appearance::set_zoom(next, &store, cx) {
        return;
    }
    let text = toast_text(
        appearance::zoom_percent(previous),
        appearance::zoom_percent(next),
    );
    app.update(cx, |state, cx| state.raise_notice(text, None, cx));
}

fn zoom_button(
    app: &Entity<AppState>,
    id: &'static str,
    glyph: &'static str,
    label: &'static str,
    change: WindowZoom,
    enabled: bool,
) -> Stateful<Div> {
    let app = app.clone();
    div()
        .id(id)
        .size(px(BUTTON_SIZE))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(BUTTON_RADIUS))
        .child(symbol(glyph, BUTTON_ICON, theme::text()))
        .tooltip(move |window, cx| Tooltip::new(label).build(window, cx))
        .when(!enabled, |button| button.opacity(DISABLED_OPACITY))
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(|button| button.bg(theme::popup_hover()))
        })
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            if enabled {
                change_zoom(&app, change, cx);
            }
        })
}

pub fn menu_row(app: &Entity<AppState>, cx: &App) -> Div {
    let zoom = active_appearance::current(cx).zoom;
    let zoomed = appearance::zoom_percent(zoom) != DEFAULT_PERCENT;
    h_flex()
        .w_full()
        .gap(px(8.))
        .items_center()
        .justify_between()
        .child(div().text_color(theme::text()).child(ZOOM_LABEL))
        .child(
            h_flex()
                .flex_none()
                .gap(px(2.))
                .items_center()
                .child(zoom_button(
                    app,
                    "window-zoom-out",
                    "zoom_out",
                    ZOOM_OUT_LABEL,
                    WindowZoom::Out,
                    true,
                ))
                .child(
                    div()
                        .debug_selector(|| "window-zoom-level".into())
                        .text_color(theme::text_muted())
                        .child(menu_level(zoom)),
                )
                .child(zoom_button(
                    app,
                    "window-zoom-in",
                    "zoom_in",
                    ZOOM_IN_LABEL,
                    WindowZoom::In,
                    true,
                ))
                .child(zoom_button(
                    app,
                    "window-zoom-reset",
                    "refresh",
                    ZOOM_RESET_LABEL,
                    WindowZoom::Reset,
                    zoomed,
                )),
        )
}

#[cfg(test)]
mod tests {
    use super::{WindowZoom, menu_level, toast_text};

    #[test]
    fn toast_texts_follow_the_teams_zoom_toast() {
        assert_eq!(toast_text(100, 110), "110% \u{b7} Zoomed in");
        assert_eq!(toast_text(110, 100), "100% \u{b7} Default size");
        assert_eq!(toast_text(90, 80), "80% \u{b7} Zoomed out");
        assert_eq!(toast_text(67, 50), "50% \u{b7} Minimum zoom");
        assert_eq!(toast_text(300, 400), "400% \u{b7} Maximum zoom");
    }

    #[test]
    fn the_menu_rounds_the_level_to_five_like_teams() {
        assert_eq!(menu_level(1.), "(100)%");
        assert_eq!(menu_level(0.67), "(65)%");
        assert_eq!(menu_level(1.75), "(175)%");
    }

    #[test]
    fn reset_always_returns_to_the_default_size() {
        assert_eq!(WindowZoom::Reset.next(2.5), 1.);
        assert_eq!(WindowZoom::In.next(1.), 1.1);
        assert_eq!(WindowZoom::Out.next(1.), 0.9);
    }
}
