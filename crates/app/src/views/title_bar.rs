use std::sync::{Arc, OnceLock};

use gpui_kit::assets::IconName;
use gpui_kit::component::{TitleBar, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::avatar::{person_avatar, with_presence};
use super::widgets::{count_badge, icon, symbol};
use crate::data::Directory;
use crate::theme;

pub const TITLE_BAR_HEIGHT: f32 = 40.;
const SEARCH_WIDTH: f32 = 380.;
const SEARCH_HINT: &str = "Search chats, people and channels";
const SHORTCUT_HINT: &str = "Ctrl K";
const OWN_AVATAR_SIZE: f32 = 26.;
const APP_ICON_SIZE: f32 = 18.;
const APP_ICON_PNG: &[u8] = include_bytes!("../../assets/icon/teams-fast-256.png");

fn app_icon() -> Arc<Image> {
    static ICON: OnceLock<Arc<Image>> = OnceLock::new();
    ICON.get_or_init(|| Arc::new(Image::from_bytes(ImageFormat::Png, APP_ICON_PNG.to_vec())))
        .clone()
}

pub fn render_title_bar(
    directory: &Directory,
    activity_unread: usize,
    on_search: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_saved: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_notifications: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let saved = div()
        .id("title-saved")
        .occlude()
        .size(px(28.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|button| button.bg(theme::surface()))
        .child(icon(IconName::Bookmark, 18., theme::text_muted()))
        .on_click(on_saved);
    let notifications = div()
        .id("title-notifications")
        .relative()
        .occlude()
        .size(px(28.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|button| button.bg(theme::surface()))
        .child(symbol("notifications", 18., theme::text_muted()))
        .when(activity_unread > 0, |bell| {
            bell.child(
                div()
                    .absolute()
                    .top(px(-4.))
                    .right(px(-6.))
                    .child(count_badge(
                        activity_unread.min(u32::MAX as usize) as u32,
                        false,
                    )),
            )
        })
        .on_click(on_notifications);
    let search = h_flex()
        .id("title-search")
        .occlude()
        .w(px(SEARCH_WIDTH))
        .h(px(28.))
        .px(px(10.))
        .gap(px(8.))
        .items_center()
        .rounded(px(6.))
        .bg(theme::surface())
        .border_1()
        .border_color(theme::border())
        .cursor_pointer()
        .text_size(px(12.))
        .text_color(theme::text_muted())
        .hover(|field| field.border_color(theme::border_strong()))
        .child(icon(IconName::Search, 14., theme::text_muted()))
        .child(div().flex_1().child(SEARCH_HINT))
        .child(
            div()
                .px(px(6.))
                .py(px(1.))
                .rounded(px(4.))
                .border_1()
                .border_color(theme::border_strong())
                .text_size(px(11.))
                .child(SHORTCUT_HINT),
        )
        .on_click(on_search);
    let own = directory.me.as_ref().map(|me| {
        let avatar = person_avatar(
            directory,
            Some(&me.user_id),
            &me.display_name,
            OWN_AVATAR_SIZE,
        );
        with_presence(
            avatar,
            directory.presence_of(&me.user_id),
            OWN_AVATAR_SIZE,
            theme::background(),
        )
    });
    TitleBar::new()
        .h(px(TITLE_BAR_HEIGHT))
        .bg(theme::background())
        .child(
            h_flex()
                .flex_1()
                .h_full()
                .gap(px(12.))
                .items_center()
                .child(img(app_icon()).size(px(APP_ICON_SIZE)).flex_none())
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::text())
                        .child(crate::APP_NAME),
                )
                .child(div().flex_1().flex().justify_center().child(search))
                .child(saved)
                .child(notifications)
                .children(own)
                .child(div().w(px(4.))),
        )
}
