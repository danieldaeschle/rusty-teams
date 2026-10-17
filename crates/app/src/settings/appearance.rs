use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::rows::{Choice, ROW_INSET, caption, control_row, select_with_width, titled};
use crate::active_appearance;
use crate::app_state::AppState;
use crate::appearance::{AppIcon, ThemeChoice};
use crate::theme::{self, Palette};
use crate::views::title_bar::app_icon_image;

const TILE_WIDTH: f32 = 168.;
const TILE_PADDING: f32 = 8.;
const TILE_GAP: f32 = 12.;
const TILE_RADIUS: f32 = 12.;
const PREVIEW_HEIGHT: f32 = 96.;
const PREVIEW_RADIUS: f32 = TILE_RADIUS - TILE_PADDING;
const PREVIEW_ICON_SIZE: f32 = 48.;
const RADIO_SIZE: f32 = 16.;
const RADIO_DOT_SIZE: f32 = 8.;
const COLOR_SCHEME_DETAIL: &str = "The colors to use for Rusty Teams";
const THEME_TITLE: &str = "Theme";
const THEME_DETAIL: &str = "The theme to use for Teams";
const THEME_SELECT_WIDTH: f32 = 240.;
#[cfg(windows)]
const APP_ICON_DETAIL: &str =
    "Shown in the title bar, taskbar and tray. Pinned shortcuts keep the icon of the .exe";
#[cfg(not(windows))]
const APP_ICON_DETAIL: &str = "Shown in the title bar, taskbar and tray";

pub fn render(app: &Entity<AppState>, cx: &App) -> Div {
    let current = active_appearance::current(cx);
    let scheme_tiles = theme::SCHEMES.into_iter().map(|scheme| {
        let app = app.clone();
        tile(
            SharedString::from(format!("settings-color-scheme-{}", scheme.id)),
            scheme.name,
            scheme.id == current.color_scheme.id,
            scheme_preview(scheme.palette(theme::shade())),
        )
        .on_click(move |_, _, cx| {
            let store = app.read(cx).store.clone();
            active_appearance::set_color_scheme(scheme, &store, cx);
        })
    });
    let icon_tiles = AppIcon::ALL.into_iter().map(|icon| {
        let app = app.clone();
        tile(
            SharedString::from(format!("settings-app-icon-{}", icon.key())),
            icon.label(),
            icon == current.app_icon,
            icon_preview(icon),
        )
        .on_click(move |_, _, cx| {
            let store = app.read(cx).store.clone();
            active_appearance::set_app_icon(icon, &store, cx);
        })
    });
    let theme_choices = ThemeChoice::ALL
        .into_iter()
        .map(|choice| {
            let app = app.clone();
            Choice::new(choice.label(), choice == current.theme, move |cx| {
                let store = app.read(cx).store.clone();
                active_appearance::set_theme(choice, &store, cx);
            })
        })
        .collect();
    v_flex()
        .w_full()
        .child(caption("THEME"))
        .child(control_row(
            THEME_TITLE,
            THEME_DETAIL,
            select_with_width(
                "settings-theme",
                current.theme.label(),
                THEME_SELECT_WIDTH,
                false,
                theme_choices,
            ),
        ))
        .child(choice_group(
            "Color scheme",
            COLOR_SCHEME_DETAIL,
            scheme_tiles,
        ))
        .child(caption("APP ICON"))
        .child(choice_group("App icon", APP_ICON_DETAIL, icon_tiles))
}

fn choice_group(
    title: &'static str,
    detail: &'static str,
    tiles: impl Iterator<Item = Stateful<Div>>,
) -> Div {
    v_flex()
        .w_full()
        .px(px(ROW_INSET))
        .py(px(12.))
        .gap(px(12.))
        .child(titled(title, detail, theme::text_muted()))
        .child(
            h_flex()
                .w_full()
                .flex_wrap()
                .gap(px(TILE_GAP))
                .children(tiles),
        )
}

fn tile(id: SharedString, label: &'static str, selected: bool, preview: Div) -> Stateful<Div> {
    v_flex()
        .id(id)
        .w(px(TILE_WIDTH))
        .flex_none()
        .p(px(TILE_PADDING))
        .gap(px(TILE_PADDING))
        .rounded(px(TILE_RADIUS))
        .bg(theme::surface_raised())
        .cursor_pointer()
        .hover(|tile| tile.bg(theme::popup_hover()))
        .child(preview)
        .child(
            h_flex()
                .w_full()
                .px(px(TILE_PADDING / 2.))
                .pb(px(TILE_PADDING / 2.))
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .text_color(theme::text())
                        .child(label),
                )
                .child(radio(selected)),
        )
}

fn radio(selected: bool) -> Div {
    div()
        .size(px(RADIO_SIZE))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .border_1()
        .border_color(if selected {
            theme::accent()
        } else {
            theme::text_muted()
        })
        .when(selected, |radio| {
            radio.child(
                div()
                    .size(px(RADIO_DOT_SIZE))
                    .rounded_full()
                    .bg(theme::accent()),
            )
        })
}

fn preview_frame(background: Hsla) -> Div {
    div()
        .w_full()
        .h(px(PREVIEW_HEIGHT))
        .rounded(px(PREVIEW_RADIUS))
        .overflow_hidden()
        .bg(background)
}

fn scheme_preview(palette: &Palette) -> Div {
    let fill = |hex: u32| theme::color(hex);
    let bubble = |hex: u32, width: f32| div().w(px(width)).h(px(12.)).rounded(px(6.)).bg(fill(hex));
    preview_frame(fill(palette.background)).child(
        h_flex()
            .size_full()
            .p(px(6.))
            .gap(px(6.))
            .child(
                v_flex()
                    .w(px(14.))
                    .h_full()
                    .gap(px(4.))
                    .items_center()
                    .child(
                        div()
                            .size(px(10.))
                            .rounded(px(3.))
                            .bg(fill(palette.surface_raised)),
                    )
                    .child(div().size(px(10.)).rounded(px(3.)).bg(fill(palette.accent))),
            )
            .child(
                v_flex()
                    .flex_1()
                    .h_full()
                    .p(px(8.))
                    .gap(px(6.))
                    .rounded(px(6.))
                    .bg(fill(palette.surface))
                    .child(h_flex().child(bubble(palette.bubble_other, 64.)))
                    .child(h_flex().justify_end().child(bubble(palette.bubble_own, 52.)))
                    .child(h_flex().child(bubble(palette.bubble_other, 40.)))
                    .child(
                        h_flex()
                            .justify_end()
                            .child(div().size(px(12.)).rounded_full().bg(fill(palette.accent))),
                    ),
            ),
    )
}

fn icon_preview(icon: AppIcon) -> Div {
    preview_frame(theme::background())
        .flex()
        .items_center()
        .justify_center()
        .child(img(app_icon_image(icon)).size(px(PREVIEW_ICON_SIZE)))
}
