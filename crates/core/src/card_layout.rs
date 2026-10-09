use serde_json::Value;

use crate::adaptive_card::{
    ContainerStyle, VerticalAlignment, bool_field, is_supported_url, parse_container_style,
    parse_vertical_alignment, pixel_field, string_field,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HorizontalAlignment {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContainerLayout {
    pub style: ContainerStyle,
    pub min_height: Option<f32>,
    pub vertical_alignment: VerticalAlignment,
    pub background_image: Option<String>,
    pub bleed: bool,
    pub rtl: bool,
}

impl ContainerLayout {
    pub fn parse(value: &Value) -> ContainerLayout {
        ContainerLayout {
            style: parse_container_style(string_field(value, "style")),
            min_height: pixel_field(value, "minHeight"),
            vertical_alignment: parse_vertical_alignment(string_field(
                value,
                "verticalContentAlignment",
            )),
            background_image: parse_background_image(value),
            bleed: bool_field(value, "bleed"),
            rtl: bool_field(value, "rtl"),
        }
    }
}

pub(crate) fn parse_horizontal_alignment(value: Option<&str>) -> Option<HorizontalAlignment> {
    match value.map(str::to_ascii_lowercase).as_deref() {
        Some("left") => Some(HorizontalAlignment::Left),
        Some("center") => Some(HorizontalAlignment::Center),
        Some("right") => Some(HorizontalAlignment::Right),
        _ => None,
    }
}

fn parse_background_image(value: &Value) -> Option<String> {
    let image = value.get("backgroundImage")?;
    let url = image.as_str().or_else(|| string_field(image, "url"))?;
    is_supported_url(url).then(|| url.to_owned())
}

/// `#RRGGBB` or `#AARRGGBB` as `0xRRGGBBAA`.
pub(crate) fn parse_color_hex(value: &str) -> Option<u32> {
    let digits = value.trim().strip_prefix('#')?;
    let parsed = u32::from_str_radix(digits, 16).ok()?;
    match digits.len() {
        6 => Some(parsed << 8 | 0xff),
        8 => Some(parsed.rotate_left(8)),
        _ => None,
    }
}
