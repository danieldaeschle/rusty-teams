use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, tooltip::Tooltip};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use app_ui_kit::layout::ROW_PADDING;

use super::message_actions::Action;
use super::widgets::icon;
use crate::theme;

const BANNER_HEIGHT: f32 = 40.;
const BANNER_INSET: f32 = ROW_PADDING;
pub const PIN_BANNER_TAG: &str = "shell/islands/main/pin-banner";
const BANNER_GAP: f32 = 8.;
const BANNER_PADDING: f32 = 12.;
const BUTTON_SIZE: f32 = 28.;
const FALLBACK_PREVIEW: &str = "Pinned message";

pub struct PinBannerData {
    pub author: Option<String>,
    pub preview: Option<String>,
    pub position: usize,
    pub total: usize,
}

pub struct PinBannerActions {
    pub open: Action,
    pub previous: Action,
    pub next: Action,
    pub unpin: Action,
}

pub fn pin_banner_inset(shown: bool) -> Pixels {
    px(if shown {
        BANNER_INSET + BANNER_HEIGHT + BANNER_GAP
    } else {
        0.
    })
}

pub fn pin_banner_overlay(banner: AnyElement) -> AnyElement {
    div()
        .absolute()
        .top(px(BANNER_INSET))
        .left(px(BANNER_INSET))
        .right(px(BANNER_INSET))
        .child(banner)
        .into_any_element()
}

pub fn cycle_pin(index: usize, length: usize, delta: isize) -> usize {
    if length == 0 {
        return 0;
    }
    (index as isize + delta).rem_euclid(length as isize) as usize
}

pub fn position_label(position: usize, total: usize) -> String {
    format!("{} of {}", position + 1, total)
}

fn icon_button(id: &'static str, glyph: IconName, action: Action) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(BUTTON_SIZE))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(7.))
        .cursor_pointer()
        .hover(|button| button.bg(theme::popup_hover()))
        .child(icon(glyph, 14., theme::text_muted()))
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            action(window, cx);
        })
}

pub fn render_pin_banner(data: PinBannerData, actions: PinBannerActions) -> AnyElement {
    let PinBannerData {
        author,
        preview,
        position,
        total,
    } = data;
    let PinBannerActions {
        open,
        previous,
        next,
        unpin,
    } = actions;
    h_flex()
        .id("pin-banner")
        .debug_selector(|| PIN_BANNER_TAG.into())
        .group("pin-banner")
        .occlude()
        .w_full()
        .h(px(BANNER_HEIGHT))
        .pl(px(BANNER_PADDING))
        .pr(px(6.))
        .gap(px(8.))
        .items_center()
        .rounded(px(8.))
        .bg(theme::row_hover())
        .shadow(theme::toast_shadow())
        .cursor_pointer()
        .hover(|banner| banner.bg(theme::surface_raised()))
        .on_click(move |_, window, cx| open(window, cx))
        .child(icon(IconName::Pin, 14., theme::accent_text()))
        .child(
            div()
                .flex_none()
                .text_size(px(12.))
                .text_color(theme::text_muted())
                .child("Pinned"),
        )
        .when_some(author, |banner, author| {
            banner.child(
                div()
                    .flex_none()
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text())
                    .child(author),
            )
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(13.))
                .text_color(theme::text_soft())
                .child(preview.unwrap_or_else(|| FALLBACK_PREVIEW.to_owned())),
        )
        .when(total > 1, |banner| {
            banner
                .child(
                    div()
                        .flex_none()
                        .text_size(px(12.))
                        .text_color(theme::text_muted())
                        .child(position_label(position, total)),
                )
                .child(icon_button("pin-previous", IconName::ChevronUp, previous))
                .child(icon_button("pin-next", IconName::ChevronDown, next))
        })
        .child(
            icon_button("pin-unpin", IconName::X, unpin)
                .opacity(0.)
                .group_hover("pin-banner", |button| button.opacity(1.))
                .tooltip(|window, cx| Tooltip::new("Unpin for everyone").build(window, cx)),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{BANNER_HEIGHT, cycle_pin, pin_banner_inset, position_label};
    use gpui_kit::px;

    #[test]
    fn cycling_wraps_around_both_ends() {
        assert_eq!(cycle_pin(0, 3, 1), 1);
        assert_eq!(cycle_pin(2, 3, 1), 0);
        assert_eq!(cycle_pin(0, 3, -1), 2);
        assert_eq!(cycle_pin(1, 3, -1), 0);
    }

    #[test]
    fn cycling_an_empty_or_single_list_stays_put() {
        assert_eq!(cycle_pin(0, 0, 1), 0);
        assert_eq!(cycle_pin(0, 1, 1), 0);
        assert_eq!(cycle_pin(0, 1, -1), 0);
    }

    #[test]
    fn inset_clears_the_banner_only_while_it_shows() {
        assert!(pin_banner_inset(true) > px(BANNER_HEIGHT));
        assert_eq!(pin_banner_inset(false), px(0.));
    }

    #[test]
    fn position_is_one_based() {
        assert_eq!(position_label(0, 3), "1 of 3");
    }
}
