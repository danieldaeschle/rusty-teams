use gpui_kit::component::v_flex;
use gpui_kit::*;

use super::rows::{caption, control_row, text_button};
use crate::{format, updater};

pub fn render() -> Div {
    let build_id = updater::running_version();
    let copied = build_id.to_owned();
    v_flex()
        .w_full()
        .child(caption("RUSTY TEAMS"))
        .child(control_row(
            "Version",
            format::short_version(build_id, env!("CARGO_PKG_VERSION")),
            div(),
        ))
        .child(control_row(
            "Build",
            build_id.to_owned(),
            text_button("settings-copy-build", "Copy").on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()))
            }),
        ))
}
