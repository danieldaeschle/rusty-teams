use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::v_flex;
use gpui_kit::*;

use super::widgets::icon;
use crate::theme;

pub(super) const TOOL_WIDTH: f32 = 56.;
pub(super) const TOOL_HEIGHT: f32 = 48.;
pub(super) const TOOL_GAP: f32 = 2.;
pub(super) const GROUP_GAP: f32 = 12.;
pub(super) const SPLIT_CHEVRON_WIDTH: f32 = 20.;
pub(super) const LEAVE_HEIGHT: f32 = 32.;
pub(super) const GROUP_DIVIDER_WIDTH: f32 = 1.;
const TOOL_ICON: f32 = 20.;
const TOOL_LABEL: f32 = 11.;
const TOOL_RADIUS: f32 = 6.;
const CHEVRON_ICON: f32 = 12.;

pub(super) fn tool_face(
    id: impl Into<ElementId>,
    glyph: IconName,
    label: impl Into<SharedString>,
    active: bool,
) -> Stateful<Div> {
    let color = if active {
        theme::accent_text()
    } else {
        theme::text()
    };
    v_flex()
        .id(id)
        .w(px(TOOL_WIDTH))
        .h(px(TOOL_HEIGHT))
        .flex_none()
        .items_center()
        .justify_center()
        .gap(px(3.))
        .rounded(px(TOOL_RADIUS))
        .cursor_pointer()
        .hover(|face| face.bg(theme::row_hover()))
        .child(icon(glyph, TOOL_ICON, color))
        .child(
            div()
                .text_size(px(TOOL_LABEL))
                .line_height(px(TOOL_LABEL + 3.))
                .text_color(color)
                .child(label.into()),
        )
}

pub(super) fn tool_trigger(button_id: &'static str, face: Stateful<Div>) -> Button {
    Button::new(button_id)
        .ghost()
        .p_0()
        .w(px(TOOL_WIDTH))
        .h(px(TOOL_HEIGHT))
        .rounded(px(TOOL_RADIUS))
        .child(face)
}

pub(super) fn split_chevron(button_id: &'static str, face_id: &'static str) -> Button {
    Button::new(button_id)
        .ghost()
        .p_0()
        .w(px(SPLIT_CHEVRON_WIDTH))
        .h(px(TOOL_HEIGHT))
        .rounded(px(TOOL_RADIUS))
        .child(
            div()
                .id(face_id)
                .w(px(SPLIT_CHEVRON_WIDTH))
                .h(px(TOOL_HEIGHT))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(TOOL_RADIUS))
                .hover(|face| face.bg(theme::row_hover()))
                .child(icon(IconName::ChevronDown, CHEVRON_ICON, theme::text())),
        )
}

pub(super) fn group_divider() -> Div {
    div()
        .flex_none()
        .w(px(GROUP_DIVIDER_WIDTH))
        .h(px(TOOL_HEIGHT - 16.))
        .bg(theme::border())
}
