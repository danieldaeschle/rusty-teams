use std::sync::{Arc, OnceLock};

use gpui_kit::assets::IconName;
use gpui_kit::component::{TitleBar, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::widgets::icon;
use crate::app_state::Mode;
use crate::theme;
use crate::updater::UpdateStatus;

pub const TITLE_BAR_HEIGHT: f32 = 40.;
const SEARCH_WIDTH: f32 = 380.;
const SEARCH_HINT: &str = "Search chats, people and channels";
const SHORTCUT_HINT: &str = "Ctrl K";
const APP_ICON_SIZE: f32 = 18.;
const APP_ICON_PNG: &[u8] = include_bytes!("../../assets/icon/teams-fast-256.png");

fn app_icon() -> Arc<Image> {
    static ICON: OnceLock<Arc<Image>> = OnceLock::new();
    ICON.get_or_init(|| Arc::new(Image::from_bytes(ImageFormat::Png, APP_ICON_PNG.to_vec())))
        .clone()
}

fn mode_pill(label: &'static str) -> Div {
    div()
        .debug_selector(|| "shell/title-bar/mode-pill".into())
        .flex_none()
        .px(px(8.))
        .h(px(18.))
        .flex()
        .items_center()
        .rounded_full()
        .border_1()
        .border_color(theme::amber().opacity(0.5))
        .bg(theme::amber().opacity(0.14))
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::amber())
        .child(label)
}

pub fn mode_label(mode: Mode) -> Option<&'static str> {
    if mode.demo {
        Some("Demo")
    } else if mode.read_only {
        Some("Read-only")
    } else {
        None
    }
}

pub fn update_button(
    update: &UpdateStatus,
    on_restart: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Option<AnyElement> {
    let installing = match update {
        UpdateStatus::Ready => false,
        UpdateStatus::Installing => true,
        UpdateStatus::UpToDate | UpdateStatus::Failed(_) => return None,
    };
    Some(
        div()
            .id("title-update-restart")
            .debug_selector(|| "shell/title-bar/update".into())
            .flex_none()
            .h(px(24.))
            .px(px(10.))
            .flex()
            .items_center()
            .rounded(px(6.))
            .bg(theme::accent())
            .text_color(theme::on_accent())
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .when(!installing, |button| {
                button
                    .cursor_pointer()
                    .hover(|button| button.bg(theme::accent_text()))
                    .on_click(on_restart)
            })
            .when(installing, |button| button.opacity(0.7))
            .child(if installing {
                "Restarting..."
            } else {
                "Restart to update"
            })
            .into_any_element(),
    )
}

pub fn render_title_bar(
    mode_label: Option<&'static str>,
    update: Option<AnyElement>,
    own: Option<AnyElement>,
    on_search: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let search = h_flex()
        .id("title-search")
        .debug_selector(|| "shell/title-bar/search".into())
        .occlude()
        .w(px(SEARCH_WIDTH))
        .h(px(28.))
        .px(px(10.))
        .gap(px(8.))
        .items_center()
        .rounded(px(6.))
        .bg(theme::surface())
        .cursor_pointer()
        .text_size(px(12.))
        .text_color(theme::text_muted())
        .child(icon(IconName::Search, 14., theme::text_muted()))
        .child(div().flex_1().child(SEARCH_HINT))
        .child(
            div()
                .px(px(6.))
                .py(px(1.))
                .rounded(px(4.))
                .bg(theme::surface_raised())
                .text_size(px(11.))
                .child(SHORTCUT_HINT),
        )
        .on_click(on_search);
    let bar = TitleBar::new()
        .h(px(TITLE_BAR_HEIGHT))
        .border_b_0()
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
                        .child(app_base::APP_NAME),
                )
                .children(mode_label.map(mode_pill))
                .child(div().flex_1().flex().justify_center().child(search))
                .children(update)
                .children(own)
                .child(div().w(px(4.))),
        );
    div()
        .debug_selector(|| "shell/title-bar".into())
        .flex_none()
        .w_full()
        .child(bar)
}
