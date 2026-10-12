use std::time::Duration;

use gpui_kit::component::Icon;
use gpui_kit::*;

use crate::theme;

const SPIN_PERIOD: Duration = Duration::from_millis(900);

pub fn spinner(size: f32) -> Div {
    let layer = |name: &str, color: Hsla| {
        Icon::empty()
            .path(format!("symbols/{name}.svg"))
            .size(px(size))
            .text_color(color)
    };
    div()
        .relative()
        .size(px(size))
        .flex_none()
        .child(layer("spinner_track", theme::popup_hover()))
        .child(div().absolute().top_0().left_0().child(
            layer("spinner_arc", theme::accent()).with_animation(
                "agent-spinner",
                Animation::new(SPIN_PERIOD).repeat(),
                |arc, progress| arc.rotate(percentage(progress)),
            ),
        ))
}
