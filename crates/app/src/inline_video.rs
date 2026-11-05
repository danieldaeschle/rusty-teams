use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::*;

#[cfg(windows)]
mod native_windows;
#[cfg(windows)]
use native_windows as native;
#[cfg(not(windows))]
mod native_stub;
#[cfg(not(windows))]
use native_stub as native;

use native::Native;
pub use native::available;
#[cfg(windows)]
pub use native::configure;

const PERSONAL_SITE: &str = "/personal/";
const TEAM_SITES: [&str; 2] = ["/sites/", "/teams/"];
const PLAYER_OPTIONS: &str = "%7B%22ust%22%3Atrue%2C%22hv%22%3A%22CopyEmbedCode%22%7D";

/// SharePoint's embeddable player for a video stored in OneDrive or a team site.
pub fn player_url(content_url: &str) -> Option<String> {
    let rest = content_url.strip_prefix("https://")?;
    let (host, path) = rest.split_once('/')?;
    if !host.ends_with(".sharepoint.com") {
        return None;
    }
    let path = format!("/{}", path.split(['?', '#']).next()?);
    let site_prefix = std::iter::once(PERSONAL_SITE)
        .chain(TEAM_SITES)
        .find(|prefix| path.starts_with(prefix))?;
    let site_name = path[site_prefix.len()..].split('/').next()?;
    if site_name.is_empty() || path.len() <= site_prefix.len() + site_name.len() + 1 {
        return None;
    }
    let site = format!("https://{host}{site_prefix}{site_name}");
    Some(format!(
        "{site}/_layouts/15/embed.aspx?id={path}&embed={PLAYER_OPTIONS}"
    ))
}

struct Playing {
    key: SharedString,
    window: WindowId,
    native: Native,
    painted_in: Rc<Cell<u64>>,
}

#[derive(Default)]
pub struct InlineVideo {
    playing: Option<Playing>,
    frame: Rc<Cell<u64>>,
}

impl Global for InlineVideo {}

pub fn play(key: SharedString, content_url: &str, window: &mut Window, cx: &mut App) -> bool {
    let Some(url) = player_url(content_url).filter(|_| available()) else {
        return false;
    };
    stop(cx);
    let Some((native, mut new_windows)) = Native::open(&url, window) else {
        return false;
    };
    cx.spawn(async move |cx| {
        while let Some(target) = new_windows.recv().await {
            cx.update(|cx| cx.open_url(&target));
        }
    })
    .detach();
    let state = cx.default_global::<InlineVideo>();
    state.playing = Some(Playing {
        key,
        window: window.window_handle().window_id(),
        native,
        painted_in: Rc::new(Cell::new(u64::MAX)),
    });
    window.refresh();
    true
}

pub fn stop(cx: &mut App) {
    if !cx.has_global::<InlineVideo>() {
        return;
    }
    if let Some(playing) = cx.global_mut::<InlineVideo>().playing.take() {
        playing.native.close();
    }
}

/// Keeps the player on the slot while the slot is painted.
pub fn slot(key: SharedString) -> impl IntoElement {
    canvas(
        move |bounds, window, cx| {
            let Some(state) = cx.try_global::<InlineVideo>() else {
                return;
            };
            let Some(playing) = state.playing.as_ref().filter(|playing| playing.key == key) else {
                return;
            };
            playing.painted_in.set(state.frame.get());
            let visible = window.content_mask().bounds.intersect(&bounds);
            let hidden: Vec<Bounds<Pixels>> = clipped_parts(bounds, visible);
            playing.native.place(
                bounds,
                &hidden,
                window.scale_factor(),
                visible.size.height > px(0.),
            );
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
}

/// Paints after the timeline: closes a player whose slot was not painted this frame.
pub fn sentinel() -> impl IntoElement {
    canvas(
        |_, window, cx| {
            let window_id = window.window_handle().window_id();
            let Some(state) = cx.try_global::<InlineVideo>() else {
                return false;
            };
            let frame = state.frame.get();
            state.frame.set(frame.wrapping_add(1));
            state.playing.as_ref().is_some_and(|playing| {
                playing.window == window_id && playing.painted_in.get() != frame
            })
        },
        |_, orphaned, _, cx| {
            if orphaned {
                stop(cx);
            }
        },
    )
    .absolute()
    .size_0()
}

fn clipped_parts(bounds: Bounds<Pixels>, visible: Bounds<Pixels>) -> Vec<Bounds<Pixels>> {
    if visible.size.height <= px(0.) || visible.size.width <= px(0.) {
        return Vec::new();
    }
    let mut parts = Vec::new();
    if visible.top() > bounds.top() {
        parts.push(Bounds::from_corners(
            bounds.origin,
            point(bounds.right(), visible.top()),
        ));
    }
    if visible.bottom() < bounds.bottom() {
        parts.push(Bounds::from_corners(
            point(bounds.left(), visible.bottom()),
            bounds.bottom_right(),
        ));
    }
    parts
}

#[cfg(test)]
mod tests {
    use gpui_kit::{Bounds, point, px, size};

    use super::{clipped_parts, player_url};

    #[test]
    fn a_onedrive_chat_video_plays_in_the_personal_site_player() {
        let url = player_url(
            "https://contoso-my.sharepoint.com/personal/ann_contoso_com/Documents/Chat%20Files/clip.mp4",
        );
        let expected = "https://contoso-my.sharepoint.com/personal/ann_contoso_com/_layouts/15/embed.aspx?id=/personal/ann_contoso_com/Documents/Chat%20Files/clip.mp4&embed=%7B%22ust%22%3Atrue%2C%22hv%22%3A%22CopyEmbedCode%22%7D";
        assert_eq!(url.as_deref(), Some(expected));
    }

    #[test]
    fn a_channel_video_plays_in_the_team_site_player() {
        let url = player_url(
            "https://contoso.sharepoint.com/sites/Dev/Shared%20Documents/General/demo.mp4?web=1",
        )
        .unwrap();
        assert!(url.starts_with("https://contoso.sharepoint.com/sites/Dev/_layouts/15/embed.aspx?id=/sites/Dev/Shared%20Documents/General/demo.mp4&"));
    }

    #[test]
    fn other_hosts_have_no_inline_player() {
        assert_eq!(player_url("https://example.com/personal/a/clip.mp4"), None);
        assert_eq!(player_url("https://contoso.sharepoint.com/clip.mp4"), None);
        assert_eq!(
            player_url("https://contoso-my.sharepoint.com/personal/"),
            None
        );
    }

    #[test]
    fn only_the_parts_outside_the_clip_are_cut_away() {
        let bounds = Bounds::new(point(px(0.), px(0.)), size(px(100.), px(100.)));
        let visible = Bounds::new(point(px(0.), px(30.)), size(px(100.), px(50.)));
        let parts = clipped_parts(bounds, visible);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].size.height, px(30.));
        assert_eq!(parts[1].origin.y, px(80.));
        assert!(clipped_parts(bounds, bounds).is_empty());
    }
}
