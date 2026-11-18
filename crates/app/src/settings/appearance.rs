use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::font_picker::{FontPicker, FontTarget};
use super::rows::{
    Choice, ROW_INSET, caption, control_row, select_with_width, text_button, titled,
};
use crate::active_appearance;
use crate::active_locale;
use crate::app_state::AppState;
use crate::appearance::{AppIcon, ThemeChoice};
use crate::i18n::tr;
use crate::theme::{self, AccentPreset, Palette};
use crate::views::title_bar::app_icon_image;
use app_base::ui_locale::LocalePreference;

const TILE_WIDTH: f32 = 168.;
const TILE_PADDING: f32 = 8.;
const TILE_GAP: f32 = 12.;
const TILE_RADIUS: f32 = 12.;
const PREVIEW_HEIGHT: f32 = 96.;
const PREVIEW_RADIUS: f32 = TILE_RADIUS - TILE_PADDING;
const PREVIEW_ICON_SIZE: f32 = 48.;
const RADIO_SIZE: f32 = 16.;
const RADIO_DOT_SIZE: f32 = 8.;
const THEME_SELECT_WIDTH: f32 = 240.;
const LANGUAGE_SELECT_WIDTH: f32 = 240.;
const SWATCH_SIZE: f32 = 24.;
const SWATCH_DOT_SIZE: f32 = 8.;
const SWATCH_GAP: f32 = 8.;

fn language_label(pref: LocalePreference) -> String {
    match pref {
        LocalePreference::Auto => tr("settings.appearance.language.follow_system"),
        LocalePreference::English => tr("settings.appearance.language.english"),
        LocalePreference::German => tr("settings.appearance.language.german"),
    }
}
fn accent_label(preset: AccentPreset) -> String {
    match preset {
        AccentPreset::Rust => tr("settings.appearance.accent_preset.rust"),
        AccentPreset::Amber => tr("settings.appearance.accent_preset.amber"),
        AccentPreset::Emerald => tr("settings.appearance.accent_preset.emerald"),
        AccentPreset::Cyan => tr("settings.appearance.accent_preset.cyan"),
        AccentPreset::Blue => tr("settings.appearance.accent_preset.blue"),
        AccentPreset::Violet => tr("settings.appearance.accent_preset.violet"),
        AccentPreset::Pink => tr("settings.appearance.accent_preset.pink"),
        AccentPreset::Red => tr("settings.appearance.accent_preset.red"),
    }
}

fn app_icon_detail() -> String {
    #[cfg(windows)]
    {
        tr("settings.appearance.app_icon_detail_windows")
    }
    #[cfg(not(windows))]
    {
        tr("settings.appearance.app_icon_detail")
    }
}

pub fn render(app: &Entity<AppState>, picker: &Entity<FontPicker>, cx: &App) -> Div {
    let current = active_appearance::current(cx);
    let locale_pref = active_locale::current_preference(cx);
    let language_choices = LocalePreference::ALL
        .into_iter()
        .map(|pref| {
            let app = app.clone();
            Choice::new(language_label(pref), pref == locale_pref, move |cx| {
                let store = app.read(cx).store.clone();
                active_locale::set_preference(pref, &store, cx);
            })
        })
        .collect();
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
    let uses_accent = current.color_scheme.id == theme::default_scheme().id;
    let accent_swatches = AccentPreset::ALL.into_iter().map(|preset| {
        let app = app.clone();
        accent_swatch(preset, preset == current.accent).on_click(move |_, _, cx| {
            let store = app.read(cx).store.clone();
            active_appearance::set_accent(preset, &store, cx);
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
        .child(caption(&tr("settings.caption.language")))
        .child(control_row(
            &tr("settings.appearance.language.title"),
            &tr("settings.appearance.language.detail"),
            select_with_width(
                "settings-language",
                language_label(locale_pref),
                LANGUAGE_SELECT_WIDTH,
                false,
                language_choices,
            ),
        ))
        .child(caption(tr("settings.caption.regional_format")))
        .children(super::regional::rows(app, cx))
        .child(caption(tr("settings.caption.theme")))
        .child(control_row(
            tr("settings.appearance.theme"),
            tr("settings.appearance.theme_detail"),
            select_with_width(
                "settings-theme",
                current.theme.label(),
                THEME_SELECT_WIDTH,
                false,
                theme_choices,
            ),
        ))
        .child(choice_group(
            tr("settings.appearance.color_scheme"),
            tr("settings.appearance.color_scheme_detail"),
            scheme_tiles,
        ))
        .when(uses_accent, |column| {
            column.child(control_row(
                tr("settings.appearance.accent"),
                tr("settings.appearance.accent_detail"),
                h_flex()
                    .flex_none()
                    .gap(px(SWATCH_GAP))
                    .children(accent_swatches),
            ))
        })
        .child(caption(tr("settings.caption.text")))
        .child(font_row(
            FontTarget::Interface,
            tr("settings.appearance.ui_font"),
            tr("settings.appearance.ui_font_detail"),
            current.ui_font.clone(),
            picker,
            cx,
        ))
        .child(font_row(
            FontTarget::Code,
            tr("settings.appearance.code_font"),
            tr("settings.appearance.code_font_detail"),
            current.code_font.clone(),
            picker,
            cx,
        ))
        .child(caption(tr("settings.caption.app_icon")))
        .child(choice_group(
            tr("settings.appearance.app_icon"),
            app_icon_detail(),
            icon_tiles,
        ))
}

fn font_row(
    target: FontTarget,
    title: String,
    detail: String,
    font: Option<SharedString>,
    picker: &Entity<FontPicker>,
    cx: &App,
) -> Div {
    let id = match target {
        FontTarget::Interface => "settings-ui-font",
        FontTarget::Code => "settings-code-font",
    };
    let label = font.unwrap_or_else(|| tr("settings.appearance.default_font").into());
    let open_list = {
        let picker = picker.clone();
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            picker.update(cx, |picker, cx| picker.toggle(target, window, cx));
        }
    };
    control_row(
        title,
        detail,
        v_flex()
            .flex_none()
            .items_end()
            .child(text_button(id, label).on_click(open_list))
            .when(picker.read(cx).target() == Some(target), |column| {
                column.child(picker.clone())
            }),
    )
}

fn choice_group(
    title: impl Into<SharedString>,
    detail: impl Into<SharedString>,
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
                        .text_size(theme::text_body())
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

fn accent_swatch(preset: AccentPreset, selected: bool) -> Stateful<Div> {
    let palette = preset.palette(theme::shade());
    let label: SharedString = accent_label(preset).into();
    div()
        .id(SharedString::from(format!(
            "settings-accent-{}",
            preset.key()
        )))
        .size(px(SWATCH_SIZE))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .bg(theme::color(palette.accent))
        .hover(|swatch| swatch.opacity(0.85))
        .tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx))
        .when(selected, |swatch| {
            swatch.child(
                div()
                    .size(px(SWATCH_DOT_SIZE))
                    .rounded_full()
                    .bg(theme::color(palette.on_accent)),
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
                    .child(
                        h_flex()
                            .justify_end()
                            .child(bubble(palette.bubble_own, 52.)),
                    )
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
