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

const OPENABLE_SCHEMES: [&str; 5] = ["https://", "http://", "mailto:", "tel:", "msteams:"];

pub fn is_openable_link(url: &str) -> bool {
    let lower = url.trim_start().to_ascii_lowercase();
    OPENABLE_SCHEMES
        .iter()
        .any(|scheme| lower.starts_with(scheme))
}

pub fn open_link(url: &str, cx: &mut App) {
    if !is_openable_link(url) {
        return;
    }
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

#[cfg(test)]
mod tests {
    use super::is_openable_link;

    #[test]
    fn only_web_mail_phone_and_teams_links_open() {
        assert!(is_openable_link("https://example.test/a"));
        assert!(is_openable_link(" HTTP://example.test"));
        assert!(is_openable_link("mailto:someone@example.test"));
        assert!(is_openable_link("msteams:/l/chat/0/0"));
        assert!(!is_openable_link("file:///etc/passwd"));
        assert!(!is_openable_link("search-ms:query=x"));
        assert!(!is_openable_link("javascript:alert(1)"));
        assert!(!is_openable_link("//example.test"));
    }
}
