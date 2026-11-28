use std::time::Duration;

use gpui_kit::component::Icon;
use gpui_kit::*;

use app_base::theme;

const SPIN_PERIOD: Duration = Duration::from_millis(900);
/// A running agent sits inside the open chat. An uncapped spin repaints that
/// whole chat on every display refresh, so the arc advances at a fixed rate.
const SPIN_FPS: f32 = 12.;

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
                Animation::new(SPIN_PERIOD).repeat().with_max_fps(SPIN_FPS),
                |arc, progress| arc.rotate(percentage(progress)),
            ),
        ))
}
