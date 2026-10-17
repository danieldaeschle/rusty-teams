use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use app_base::theme;

pub const EDGE_THICKNESS: f32 = 1.;
const SCROLLED_THRESHOLD: f32 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeRule {
    Always,
    WhenScrolled,
}

pub fn content_hidden_above(offset_y: Pixels) -> bool {
    f32::from(offset_y) < -SCROLLED_THRESHOLD
}

pub fn edge_visible(rule: EdgeRule, offset_y: Pixels) -> bool {
    match rule {
        EdgeRule::Always => true,
        EdgeRule::WhenScrolled => content_hidden_above(offset_y),
    }
}

pub fn edge_divider(tag: &'static str) -> Div {
    div()
        .debug_selector(move || tag.into())
        .w_full()
        .h(px(EDGE_THICKNESS))
        .flex_none()
        .bg(theme::border())
}

pub fn with_top_edge(
    scroll_area: impl IntoElement,
    handle: &ScrollHandle,
    tag: &'static str,
) -> Div {
    let visible = edge_visible(EdgeRule::WhenScrolled, handle.offset().y);
    div()
        .relative()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .w_full()
        .child(scroll_area)
        .when(visible, |area| {
            area.child(edge_divider(tag).absolute().top_0().left_0().right_0())
        })
}

#[cfg(test)]
mod tests {
    use gpui_kit::px;

    use super::{EdgeRule, content_hidden_above, edge_visible};

    #[test]
    fn the_list_at_its_top_shows_no_line() {
        assert!(!content_hidden_above(px(0.)));
        assert!(!edge_visible(EdgeRule::WhenScrolled, px(0.)));
    }

    #[test]
    fn subpixel_jitter_at_the_top_shows_no_line() {
        assert!(!edge_visible(EdgeRule::WhenScrolled, px(-0.3)));
    }

    #[test]
    fn a_scrolled_list_shows_the_line() {
        assert!(edge_visible(EdgeRule::WhenScrolled, px(-1.)));
        assert!(edge_visible(EdgeRule::WhenScrolled, px(-480.)));
    }

    #[test]
    fn a_fixed_header_always_shows_the_line() {
        assert!(edge_visible(EdgeRule::Always, px(0.)));
        assert!(edge_visible(EdgeRule::Always, px(-20.)));
    }
}
