use crate::views::image_memory::bounded;
use gpui_kit::*;
use teams_core::{AnnouncementBanner, BannerBackground};

use crate::data::Directory;

pub const ANNOUNCEMENT_BANNER_TAG: &str = "post/announcement-banner";
pub const BANNER_COLORS: [&str; 4] = ["periwinkleBlue", "indigo", "rose", "orangeSherbet"];
pub const BANNER_HEIGHT: f32 = 112.;
pub const BANNER_RADIUS: f32 = 8.;
const HEADLINE_SIZE: f32 = 24.;
const HEADLINE_INSET: f32 = 24.;
const IMAGE_SHADE: f32 = 0.3;
const GRADIENT_ANGLE: f32 = 120.;

pub fn color_label(color: &str) -> &'static str {
    match color {
        "indigo" => "Indigo",
        "rose" => "Rose",
        "orangeSherbet" => "Orange sherbet",
        _ => "Periwinkle",
    }
}

pub fn banner_fill(color: &str) -> Background {
    let (from, to) = match color {
        "indigo" => (0x2F3185, 0x5B5FC7),
        "rose" => (0xA4264F, 0xE26D8B),
        "orangeSherbet" => (0xD8590F, 0xF5A55A),
        _ => (0x4F52B2, 0x8B8EF0),
    };
    linear_gradient(
        GRADIENT_ANGLE,
        linear_color_stop(rgb(from), 0.),
        linear_color_stop(rgb(to), 1.),
    )
}

/// The banner frame without its headline, shared with the composer.
pub fn banner_frame(background: &BannerBackground, directory: &Directory) -> Div {
    let frame = div()
        .relative()
        .w_full()
        .h(px(BANNER_HEIGHT))
        .flex()
        .items_center()
        .justify_center()
        .px(px(HEADLINE_INSET))
        .rounded(px(BANNER_RADIUS))
        .overflow_hidden();
    match background {
        BannerBackground::Color(color) => frame.bg(banner_fill(color)),
        BannerBackground::LocalImage(path) => frame
            .bg(banner_fill(""))
            .child(
                img(bounded(std::path::PathBuf::from(path)))
                    .absolute()
                    .inset_0()
                    .size_full()
                    .object_fit(ObjectFit::Cover),
            )
            .child(div().absolute().inset_0().bg(black().opacity(IMAGE_SHADE))),
        BannerBackground::Image(url) => {
            let frame = frame.bg(banner_fill(""));
            match directory.image(url) {
                Some(entry) => frame
                    .child(
                        img(bounded(entry.path.clone()))
                            .absolute()
                            .inset_0()
                            .size_full()
                            .object_fit(ObjectFit::Cover),
                    )
                    .child(div().absolute().inset_0().bg(black().opacity(IMAGE_SHADE))),
                None => frame,
            }
        }
    }
}

pub fn headline_style(text: Div) -> Div {
    text.relative()
        .text_size(px(HEADLINE_SIZE))
        .line_height(relative(1.25))
        .font_weight(FontWeight::BOLD)
        .text_color(white())
        .text_center()
}

pub fn announcement_banner(banner: &AnnouncementBanner, directory: &Directory) -> Div {
    banner_frame(&banner.background, directory)
        .debug_selector(|| ANNOUNCEMENT_BANNER_TAG.into())
        .child(headline_style(div().line_clamp(2).text_ellipsis()).child(banner.title.clone()))
}
