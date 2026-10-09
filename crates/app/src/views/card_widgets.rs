use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::{
    BadgeAppearance, BadgeShape, BadgeSize, BadgeStyle, CardBadge, CardCompoundButton,
    CardProgressBar, CardProgressRing, IconPosition, LabelPosition, RingSize, TextColor,
};

use super::adaptive_card::{CardContext, card_color, selectable};
use super::card_icon::glyph_view;
use super::widgets::icon;
use crate::theme;

const BADGE_TINT: f32 = 0.18;
const BADGE_ICON_GAP: f32 = 4.;
const BAR_HEIGHT: f32 = 6.;
const INDETERMINATE_WIDTH: f32 = 0.3;
const INDETERMINATE_PERIOD: Duration = Duration::from_millis(1400);
const RING_PERIOD: Duration = Duration::from_millis(1000);
const RING_LABEL_GAP: f32 = 8.;
const COMPOUND_PADDING: f32 = 10.;
const COMPOUND_GAP: f32 = 10.;
const COMPOUND_ICON_SIZE: f32 = 24.;
const COMPOUND_RADIUS: f32 = 6.;

struct BadgePalette {
    fill: Hsla,
    foreground: Hsla,
    border: Option<Hsla>,
}

fn badge_palette(style: BadgeStyle, appearance: BadgeAppearance) -> BadgePalette {
    let tinted = appearance == BadgeAppearance::Tint;
    let semantic = |fill: Hsla, on_fill: Hsla, tint_text: Hsla| {
        if tinted {
            BadgePalette {
                fill: fill.opacity(BADGE_TINT),
                foreground: tint_text,
                border: Some(fill.opacity(BADGE_TINT * 2.)),
            }
        } else {
            BadgePalette {
                fill,
                foreground: on_fill,
                border: None,
            }
        }
    };
    match style {
        BadgeStyle::Accent => semantic(theme::accent(), theme::on_accent(), theme::accent_text()),
        BadgeStyle::Good => semantic(theme::green(), theme::background(), theme::green()),
        BadgeStyle::Attention => semantic(theme::red(), theme::white(), theme::red_soft()),
        BadgeStyle::Warning => semantic(theme::amber(), theme::background(), theme::amber()),
        BadgeStyle::Default => neutral(theme::badge_muted(), theme::text_strong(), tinted),
        BadgeStyle::Informative => neutral(theme::surface_raised(), theme::text_soft(), tinted),
        BadgeStyle::Subtle => BadgePalette {
            fill: if tinted {
                theme::surface_raised()
            } else {
                transparent_black()
            },
            foreground: theme::text_muted(),
            border: Some(theme::border_strong()),
        },
    }
}

fn neutral(fill: Hsla, foreground: Hsla, tinted: bool) -> BadgePalette {
    BadgePalette {
        fill: if tinted { fill.opacity(0.5) } else { fill },
        foreground,
        border: tinted.then(theme::border_strong),
    }
}

fn badge_metrics(size: BadgeSize) -> (f32, f32, f32, f32) {
    match size {
        BadgeSize::Medium => (20., 12., 6., 12.),
        BadgeSize::Large => (24., 13., 8., 14.),
        BadgeSize::ExtraLarge => (32., 14., 10., 18.),
    }
}

pub(super) fn leading(element: AnyElement) -> AnyElement {
    div()
        .w_full()
        .flex()
        .flex_row()
        .child(element)
        .into_any_element()
}

pub(super) fn badge_view(badge: &CardBadge, id: &str) -> AnyElement {
    let palette = badge_palette(badge.style, badge.appearance);
    let (height, text_size, padding, icon_size) = badge_metrics(badge.size);
    let glyph = badge
        .icon
        .as_deref()
        .map(|name| glyph_view(name, icon_size, palette.foreground));
    let (icon_before, icon_after) = match badge.icon_position {
        IconPosition::Before => (glyph, None),
        IconPosition::After => (None, glyph),
    };
    let text = (!badge.text.is_empty()).then(|| badge.text.clone());
    let tooltip = badge.tooltip.clone();
    h_flex()
        .id(ElementId::Name(format!("{id}-badge").into()))
        .flex_none()
        .h(px(height))
        .min_w(px(height))
        .px(px(padding))
        .gap(px(BADGE_ICON_GAP))
        .items_center()
        .justify_center()
        .map(|frame| match badge.shape {
            BadgeShape::Square => frame.rounded(px(2.)),
            BadgeShape::Rounded => frame.rounded(px(height / 4.)),
            BadgeShape::Circular => frame.rounded_full(),
        })
        .bg(palette.fill)
        .when_some(palette.border, |frame, border| {
            frame.border_1().border_color(border)
        })
        .text_color(palette.foreground)
        .text_size(px(text_size))
        .font_weight(FontWeight::SEMIBOLD)
        .when_some(tooltip, |frame, tooltip| {
            frame.tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        })
        .children(icon_before)
        .children(text)
        .children(icon_after)
        .into_any_element()
}

pub(super) fn progress_bar_view(bar: &CardProgressBar, id: &str) -> AnyElement {
    let fill = match bar.color {
        TextColor::Default | TextColor::Dark | TextColor::Light => theme::accent(),
        other => card_color(other, false),
    };
    let track = div()
        .relative()
        .w_full()
        .h(px(BAR_HEIGHT))
        .overflow_hidden()
        .rounded_full()
        .bg(theme::border_strong());
    match bar.value {
        Some(value) => track
            .child(
                div()
                    .h_full()
                    .w(relative(value / 100.))
                    .rounded_full()
                    .bg(fill),
            )
            .into_any_element(),
        None => track
            .child(
                div()
                    .absolute()
                    .top_0()
                    .h_full()
                    .w(relative(INDETERMINATE_WIDTH))
                    .rounded_full()
                    .bg(fill)
                    .with_animation(
                        ElementId::Name(format!("{id}-pulse").into()),
                        Animation::new(INDETERMINATE_PERIOD).repeat(),
                        |segment, delta| {
                            let travel = 1. + INDETERMINATE_WIDTH;
                            segment.left(relative(
                                (delta + 0.3).fract() * travel - INDETERMINATE_WIDTH,
                            ))
                        },
                    ),
            )
            .into_any_element(),
    }
}

fn ring_pixels(size: RingSize) -> f32 {
    match size {
        RingSize::Tiny => 16.,
        RingSize::Small => 20.,
        RingSize::Medium => 28.,
        RingSize::Large => 40.,
    }
}

pub(super) fn progress_ring_view(ring: &CardProgressRing, id: &str) -> AnyElement {
    let pixels = ring_pixels(ring.size);
    let spinner = icon(IconName::Loader, pixels, theme::accent_text()).with_animation(
        ElementId::Name(format!("{id}-spin").into()),
        Animation::new(RING_PERIOD).repeat(),
        |icon, delta| icon.transform(Transformation::rotate(percentage(delta))),
    );
    let label = ring.label.clone().map(|label| {
        div()
            .text_size(px(13.))
            .text_color(theme::text_soft())
            .child(label)
    });
    let vertical = matches!(
        ring.label_position,
        LabelPosition::Above | LabelPosition::Below
    );
    let (label_before, label_after) = match ring.label_position {
        LabelPosition::Before | LabelPosition::Above => (label, None),
        LabelPosition::After | LabelPosition::Below => (None, label),
    };
    div()
        .flex()
        .items_center()
        .gap(px(RING_LABEL_GAP))
        .map(|frame| {
            if vertical {
                frame.flex_col()
            } else {
                frame.flex_row()
            }
        })
        .children(label_before)
        .child(spinner)
        .children(label_after)
        .into_any_element()
}

pub(super) fn compound_button_view(
    button: &CardCompoundButton,
    id: &str,
    context: &CardContext,
) -> AnyElement {
    let clickable = button
        .select_action
        .as_ref()
        .is_some_and(|action| action.is_clickable());
    let face = h_flex()
        .w_full()
        .items_center()
        .gap(px(COMPOUND_GAP))
        .p(px(COMPOUND_PADDING))
        .rounded(px(COMPOUND_RADIUS))
        .border_1()
        .border_color(theme::border_strong())
        .when(!clickable, |face| face.opacity(0.6))
        .children(
            button
                .icon
                .as_deref()
                .map(|name| glyph_view(name, COMPOUND_ICON_SIZE, theme::accent_text())),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w(px(0.))
                .child(
                    div()
                        .text_size(px(13.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::text_strong())
                        .child(button.title.clone()),
                )
                .children(button.description.clone().map(|description| {
                    div()
                        .text_size(px(12.))
                        .text_color(theme::text_muted())
                        .child(description)
                })),
        )
        .children(button.badge.clone().map(|badge| {
            div()
                .flex_none()
                .px(px(6.))
                .h(px(18.))
                .flex()
                .items_center()
                .rounded_full()
                .bg(theme::badge_muted())
                .text_size(px(11.))
                .font_weight(FontWeight::BOLD)
                .text_color(theme::text_strong())
                .child(badge)
        }));
    selectable(
        face.into_any_element(),
        button.select_action.as_ref(),
        id,
        false,
        context,
    )
}
