use gpui_kit::assets::IconName;
use gpui_kit::component::h_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::widgets::icon;
use crate::connection_banner::BannerText;
use crate::theme;
use app_ui_kit::layout::{ISLAND_GAP, RAIL_WIDTH};

const BANNER_HEIGHT: f32 = 32.;
const BANNER_TEXT_SIZE: f32 = 12.;
const BANNER_ICON_SIZE: f32 = 14.;

pub fn render_connection_banner(
    banner: BannerText,
    on_retry: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_leave: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let has_leave = banner.leave.is_some();
    h_flex()
        .debug_selector(|| "shell/connection-banner".into())
        .flex_none()
        .h(px(BANNER_HEIGHT))
        .ml(px(RAIL_WIDTH))
        .mr(px(ISLAND_GAP))
        .mb(px(ISLAND_GAP))
        .px(px(12.))
        .gap(px(8.))
        .items_center()
        .rounded(px(8.))
        .bg(theme::amber().opacity(0.14))
        .text_size(px(BANNER_TEXT_SIZE))
        .text_color(theme::text())
        .child(icon(
            IconName::TriangleAlert,
            BANNER_ICON_SIZE,
            theme::amber(),
        ))
        .child(div().min_w_0().truncate().child(banner.text))
        .children(banner.detail.map(|detail| {
            div()
                .flex_none()
                .text_color(theme::text_muted())
                .child(detail)
        }))
        .when_some(banner.leave, |row, label| {
            row.child(
                banner_button("connection-banner-leave", label)
                    .ml_auto()
                    .on_click(on_leave),
            )
        })
        .when_some(banner.action, |row, label| {
            row.child(
                banner_button("connection-banner-action", label)
                    .when(!has_leave, |button| button.ml_auto())
                    .on_click(on_retry),
            )
        })
}

pub(crate) fn banner_button(id: &'static str, label: String) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .px(px(8.))
        .py(px(2.))
        .rounded(px(6.))
        .cursor_pointer()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::accent_text())
        .hover(|button| button.bg(theme::row_hover()))
        .child(label)
}
