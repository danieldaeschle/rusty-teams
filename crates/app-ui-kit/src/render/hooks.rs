use gpui_kit::*;

#[derive(Clone, Copy)]
pub struct RenderHooks {
    pub open_profile_link: fn(&str, Point<Pixels>, &mut App) -> bool,
    pub jump_to_quoted: fn(String, &mut App),
    pub inline_image: fn(&str, &App) -> Option<std::path::PathBuf>,
    pub open_link: fn(&str, &mut App),
}

impl Global for RenderHooks {}

pub fn open_profile_link(url: &str, position: Point<Pixels>, cx: &mut App) -> bool {
    match cx.try_global::<RenderHooks>().copied() {
        Some(hooks) => (hooks.open_profile_link)(url, position, cx),
        None => false,
    }
}

pub fn open_link(url: &str, cx: &mut App) {
    match cx.try_global::<RenderHooks>().copied() {
        Some(hooks) => (hooks.open_link)(url, cx),
        None => cx.open_url(url),
    }
}

pub fn jump_to_quoted(message_id: String, cx: &mut App) {
    if let Some(hooks) = cx.try_global::<RenderHooks>().copied() {
        (hooks.jump_to_quoted)(message_id, cx);
    }
}

pub fn inline_image(url: &str, cx: &App) -> Option<std::path::PathBuf> {
    let hooks = cx.try_global::<RenderHooks>().copied()?;
    (hooks.inline_image)(url, cx)
}
