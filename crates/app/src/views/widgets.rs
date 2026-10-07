use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::*;

use crate::format;
use crate::sidebar_model::Unread;
use crate::theme;

pub fn icon(name: IconName, size: f32, color: Hsla) -> Icon {
    Icon::new(name).size(px(size)).text_color(color)
}

pub fn symbol(name: &str, size: f32, color: Hsla) -> Icon {
    Icon::empty()
        .path(format!("symbols/{name}.svg"))
        .size(px(size))
        .text_color(color)
}

pub fn count_badge(count: u32, muted: bool) -> Div {
    let (background, foreground) = if muted {
        (theme::badge_muted(), theme::text_strong())
    } else {
        (theme::accent(), theme::on_accent())
    };
    div()
        .min_w(px(18.))
        .h(px(18.))
        .px(px(5.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(9.))
        .bg(background)
        .text_color(foreground)
        .text_size(px(11.))
        .line_height(px(18.))
        .font_weight(FontWeight::BOLD)
        .child(format::badge_text(count))
}

pub fn dot(size: f32) -> Div {
    div()
        .size(px(size))
        .flex_none()
        .rounded_full()
        .bg(theme::accent())
}

pub fn unread_marker(unread: Unread) -> Option<Div> {
    match unread {
        Unread::None => None,
        Unread::Dot => Some(dot(8.)),
        Unread::Count(count) => Some(count_badge(count, false)),
    }
}

const SMILE_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9"/><path d="M8.5 14.5a4.5 4.5 0 0 0 7 0M9 9.5h.01M15 9.5h.01"/></svg>"#;

#[allow(dead_code)]
pub fn smile_icon(size: f32, color: Hsla) -> Icon {
    Icon::default()
        .data(SMILE_SVG)
        .size(px(size))
        .text_color(color)
}
