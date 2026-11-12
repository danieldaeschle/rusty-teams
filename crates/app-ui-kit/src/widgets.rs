use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::*;

use app_base::format;
use app_base::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unread {
    None,
    Silent,
    Dot,
    Count(u32),
}

impl Unread {
    pub fn is_unread(self) -> bool {
        self != Unread::None
    }
}

pub fn icon(name: IconName, size: f32, color: Hsla) -> Icon {
    Icon::new(name).size(px(size)).text_color(color)
}

pub fn filled_icon(name: IconName, size: f32, color: Hsla) -> Icon {
    let path = name
        .path()
        .replacen("icons/", app_base::assets::FILLED_PREFIX, 1);
    Icon::empty().path(path).size(px(size)).text_color(color)
}

pub fn menu_icon(name: IconName) -> Icon {
    Icon::new(name)
}

pub fn symbol(name: &str, size: f32, color: Hsla) -> Icon {
    Icon::empty()
        .path(format!("symbols/{name}.svg"))
        .size(px(size))
        .text_color(color)
}

#[derive(Clone, Copy)]
pub enum Corner {
    TopRight,
    BottomRight,
}

pub fn corner_badge(corner: Corner, overhang: f32) -> Div {
    let badge = div().absolute().right(px(-overhang));
    match corner {
        Corner::TopRight => badge.top(px(-overhang)),
        Corner::BottomRight => badge.bottom(px(-overhang)),
    }
}

#[derive(Clone, Copy)]
pub struct BadgeMetrics {
    pub height: f32,
    pub padding_x: f32,
    pub font_size: f32,
}

pub const COUNT_BADGE: BadgeMetrics = BadgeMetrics {
    height: 18.,
    padding_x: 5.,
    font_size: 11.,
};

pub const RAIL_BADGE: BadgeMetrics = BadgeMetrics {
    height: 16.,
    padding_x: 4.,
    font_size: 10.,
};

pub fn badge_selector(text: &str) -> String {
    format!("count-badge/{text}")
}

pub fn badge_text_selector(text: &str) -> String {
    format!("count-badge/{text}/text")
}

pub fn badge_pill(count: u32, metrics: BadgeMetrics, background: Hsla, foreground: Hsla) -> Div {
    let text = format::badge_text(count);
    div()
        .debug_selector(|| badge_selector(&text))
        .relative()
        .min_w(px(metrics.height))
        .h(px(metrics.height))
        .px(px(metrics.padding_x))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(metrics.height / 2.))
        .bg(background)
        .text_size(px(metrics.font_size))
        .line_height(px(metrics.height))
        .font_weight(FontWeight::BOLD)
        .child(
            div()
                .debug_selector(|| badge_text_selector(&text))
                .flex_none()
                .text_color(transparent_black())
                .child(text.clone()),
        )
        .child(badge_ink(text, metrics.font_size, foreground))
}

fn badge_ink(text: String, font_size: f32, color: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, cx| {
            let run = TextRun {
                len: text.len(),
                font: badge_font(window),
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let line =
                window
                    .text_system()
                    .shape_line(text.clone().into(), px(font_size), &[run], None);
            let origin = badge_text_origin(window, &text, font_size, bounds);
            let _ = line.paint(
                point(origin.x, origin.y - line.ascent),
                line.ascent + line.descent,
                TextAlign::Left,
                None,
                window,
                cx,
            );
        },
    )
    .absolute()
    .inset_0()
}

fn badge_font(window: &Window) -> Font {
    let mut font = window.text_style().font();
    font.weight = FontWeight::BOLD;
    font
}

/// Pen origin on the baseline that puts the ink centre of `text` on the centre of `bounds`.
pub fn badge_text_origin(
    window: &Window,
    text: &str,
    font_size: f32,
    bounds: Bounds<Pixels>,
) -> Point<Pixels> {
    let (ink_centre, cap_height) = badge_ink_centre(window, text, font_size);
    let centre = bounds.center();
    point(
        px(f32::from(centre.x) - ink_centre),
        px(f32::from(centre.y) + cap_height / 2.),
    )
}

/// Horizontal ink centre from the pen origin, and the cap height.
pub fn badge_ink_centre(window: &Window, text: &str, font_size: f32) -> (f32, f32) {
    let text_system = window.text_system();
    let font_id = text_system.resolve_font(&badge_font(window));
    let size = px(font_size);
    let cap_height = f32::from(text_system.cap_height(font_id, size));
    let mut pen = 0.;
    let mut ink_left = None;
    let mut ink_right = 0.;
    for character in text.chars() {
        let advance = text_system
            .advance(font_id, size, character)
            .map_or(0., |advance| f32::from(advance.width));
        if let Ok(ink) = text_system.typographic_bounds(font_id, size, character) {
            let left = pen + f32::from(ink.origin.x);
            ink_left.get_or_insert(left);
            ink_right = left + f32::from(ink.size.width);
        }
        pen += advance;
    }
    let ink_centre = ink_left.map_or(pen / 2., |left| (left + ink_right) / 2.);
    (ink_centre, cap_height)
}

pub fn count_badge(count: u32, muted: bool) -> Div {
    let (background, foreground) = if muted {
        (theme::badge_muted(), theme::text_strong())
    } else {
        (theme::accent(), theme::on_accent())
    };
    badge_pill(count, COUNT_BADGE, background, foreground)
}

pub fn page_header(title: &str) -> Div {
    div()
        .h(px(52.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .pl(px(16.))
        .pr(px(12.))
        .child(
            div()
                .flex_1()
                .text_size(px(16.))
                .font_weight(FontWeight::BOLD)
                .child(title.to_owned()),
        )
}

pub fn dot(size: f32) -> Div {
    div()
        .size(px(size))
        .flex_none()
        .rounded_full()
        .bg(theme::accent())
}

pub fn unread_marker(unread: Unread, muted: bool) -> Option<Div> {
    match unread {
        Unread::None | Unread::Silent => None,
        Unread::Dot if muted => Some(dot(8.).bg(theme::badge_muted())),
        Unread::Dot => Some(dot(8.)),
        Unread::Count(count) => Some(count_badge(count, muted)),
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
