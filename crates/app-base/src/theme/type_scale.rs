//! Text sizes of the product UI. Views use these instead of literal sizes.

use gpui_kit::{Pixels, px};

/// Timestamps, counters and other fine print.
pub fn text_caption() -> Pixels {
    px(11.)
}

/// Secondary text and hints.
pub fn text_small() -> Pixels {
    px(12.)
}

/// Controls, list rows and labels.
pub fn text_label() -> Pixels {
    px(13.)
}

/// Default reading text.
pub fn text_body() -> Pixels {
    px(14.)
}

/// Section and dialog headings.
pub fn text_heading() -> Pixels {
    px(16.)
}

/// Page titles.
pub fn text_title() -> Pixels {
    px(20.)
}
