use gpui_kit::*;

#[derive(Clone, Copy)]
pub struct RenderHooks {
    pub open_profile_link: fn(&str, Point<Pixels>, &mut App) -> bool,
    pub jump_to_quoted: fn(String, &mut App),
}

impl Global for RenderHooks {}

pub fn open_profile_link(url: &str, position: Point<Pixels>, cx: &mut App) -> bool {
    match cx.try_global::<RenderHooks>().copied() {
        Some(hooks) => (hooks.open_profile_link)(url, position, cx),
        None => false,
    }
}

pub fn jump_to_quoted(message_id: String, cx: &mut App) {
    if let Some(hooks) = cx.try_global::<RenderHooks>().copied() {
        (hooks.jump_to_quoted)(message_id, cx);
    }
}
