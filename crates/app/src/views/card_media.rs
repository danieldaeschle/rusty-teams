use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::CardMedia;

use super::widgets::icon;
use crate::theme;

const MEDIA_WIDTH: f32 = 360.;
const MEDIA_HEIGHT: f32 = 202.;
const MEDIA_RADIUS: f32 = 6.;
const PLAY_BUTTON: f32 = 52.;
const PLAY_GLYPH: f32 = 24.;
const PLAY_BACKDROP: f32 = 0.6;
const HOVER_TINT: f32 = 0.12;

pub fn media_view(media: &CardMedia, id: &str) -> AnyElement {
    let source_url = media.source_url.clone();
    let alt_text = media.alt_text.clone();
    let poster = match &media.poster_url {
        Some(url) => img(url.clone())
            .size_full()
            .rounded(px(MEDIA_RADIUS))
            .object_fit(ObjectFit::Cover)
            .with_loading(placeholder)
            .with_fallback(placeholder)
            .into_any_element(),
        None => placeholder(),
    };
    div()
        .id(ElementId::Name(format!("{id}-media").into()))
        .group(SharedString::from(format!("{id}-media")))
        .relative()
        .w(px(MEDIA_WIDTH))
        .h(px(MEDIA_HEIGHT))
        .max_w(relative(1.))
        .flex_none()
        .overflow_hidden()
        .rounded(px(MEDIA_RADIUS))
        .cursor_pointer()
        .when_some(alt_text, |frame, alt_text| {
            frame.tooltip(move |window, cx| Tooltip::new(alt_text.clone()).build(window, cx))
        })
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            cx.open_url(&source_url);
        })
        .child(poster)
        .child(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .group_hover(SharedString::from(format!("{id}-media")), |tint| {
                    tint.bg(gpui_kit::white().opacity(HOVER_TINT))
                })
                .child(
                    div()
                        .size(px(PLAY_BUTTON))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(gpui_kit::black().opacity(PLAY_BACKDROP))
                        .child(icon(IconName::Play, PLAY_GLYPH, gpui_kit::white())),
                ),
        )
        .into_any_element()
}

fn placeholder() -> AnyElement {
    div()
        .size_full()
        .bg(theme::surface_raised())
        .into_any_element()
}
