use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::CardCarousel;

use super::adaptive_card::{CardContext, ClickHandler, container_view, selectable, slot_handler};
use super::widgets::icon;
use crate::theme;

const ARROW_SIZE: f32 = 24.;
const ARROW_ICON_SIZE: f32 = 14.;
const DOT_SIZE: f32 = 7.;
const DOT_GAP: f32 = 6.;
const NAVIGATION_GAP: f32 = 8.;
const DISABLED_OPACITY: f32 = 0.35;

pub(super) fn carousel_view(
    carousel: &CardCarousel,
    id: &str,
    context: &CardContext,
) -> AnyElement {
    let page_key = format!("{id}-page");
    let last = carousel.pages.len() - 1;
    let current = context
        .state
        .slot(&page_key, carousel.initial_page)
        .min(last);
    let page = &carousel.pages[current];
    let page_id = format!("{id}-{current}");
    let content = selectable(
        container_view(&page.items, &page_id, context, &page.layout, None).into_any_element(),
        page.select_action.as_ref(),
        &page_id,
        false,
        context,
    );
    v_flex()
        .w_full()
        .gap(px(NAVIGATION_GAP))
        .child(content)
        .when(last > 0, |frame| {
            frame.child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_center()
                    .gap(px(NAVIGATION_GAP))
                    .child(arrow(
                        &format!("{id}-previous"),
                        IconName::ChevronLeft,
                        (current > 0).then(|| slot_handler(context, &page_key, current - 1)),
                    ))
                    .child(h_flex().gap(px(DOT_GAP)).children(
                        (0..=last).map(|index| dot(id, index, current, &page_key, context)),
                    ))
                    .child(arrow(
                        &format!("{id}-next"),
                        IconName::ChevronRight,
                        (current < last).then(|| slot_handler(context, &page_key, current + 1)),
                    )),
            )
        })
        .into_any_element()
}

fn arrow(arrow_id: &str, glyph: IconName, handler: Option<ClickHandler>) -> Stateful<Div> {
    div()
        .id(ElementId::Name(arrow_id.to_owned().into()))
        .size(px(ARROW_SIZE))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .border_1()
        .border_color(theme::border_strong())
        .child(icon(glyph, ARROW_ICON_SIZE, theme::text_strong()))
        .map(|button| match handler {
            Some(handler) => button
                .cursor_pointer()
                .hover(|button| button.bg(theme::row_hover()))
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    handler(cx)
                }),
            None => button
                .opacity(DISABLED_OPACITY)
                .on_click(|_, _, cx| cx.stop_propagation()),
        })
}

fn dot(
    id: &str,
    index: usize,
    current: usize,
    page_key: &str,
    context: &CardContext,
) -> Stateful<Div> {
    let handler = slot_handler(context, page_key, index);
    div()
        .id(ElementId::Name(format!("{id}-dot-{index}").into()))
        .size(px(DOT_SIZE))
        .rounded_full()
        .cursor_pointer()
        .bg(if index == current {
            theme::accent_text()
        } else {
            theme::border_strong()
        })
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            handler(cx)
        })
}
