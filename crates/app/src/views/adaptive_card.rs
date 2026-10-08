use gpui_kit::component::{h_flex, tooltip::Tooltip, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::{
    AdaptiveCard, CardAction, CardActionKind, CardColumn, CardElement, CardFact, CardImage,
    CardItem, CardSpacing, CardText, ColumnWidth, ContainerStyle, ImageSize, TextColor, TextSize,
    VerticalAlignment,
};

use crate::render::{layout_blocks, render_blocks};
use crate::theme;

const CARD_WIDTH: f32 = 480.;
const CARD_FULL_WIDTH: f32 = 720.;
const CARD_PADDING: f32 = 12.;
const CARD_RADIUS: f32 = 10.;
const CARD_BORDER_WIDTH: f32 = 1.;
const COLUMN_GAP: f32 = 8.;
const SEPARATOR_PADDING: f32 = 8.;
const ACTION_GAP: f32 = 8.;
const ACTION_RADIUS: f32 = 6.;
const ACTION_TEXT_SIZE: f32 = 13.;
const ACTION_DISABLED_OPACITY: f32 = 0.5;
const ACTION_DISABLED_TOOLTIP: &str = "Not supported in this app";
const FACT_TITLE_WIDTH: f32 = 96.;
const FACT_GAP: f32 = 2.;
const CONTAINER_PADDING: f32 = 8.;
const CONTAINER_RADIUS: f32 = 6.;
const CONTAINER_TINT: f32 = 0.16;
const TEXT_LINE_HEIGHT: f32 = 1.35;
const IMAGE_RADIUS: f32 = 4.;
const IMAGE_SET_GAP: f32 = 6.;
const SMALL_IMAGE: f32 = 40.;
const MEDIUM_IMAGE: f32 = 80.;
const LARGE_IMAGE: f32 = 160.;
const EXTERNAL_IMAGE_SCHEME: &str = "https://";

pub fn cards_view(cards: &[AdaptiveCard], id: &str, cx: &App) -> Vec<AnyElement> {
    cards
        .iter()
        .enumerate()
        .map(|(index, card)| card_view(card, &format!("{id}-card-{index}"), cx))
        .collect()
}

fn card_view(card: &AdaptiveCard, id: &str, cx: &App) -> AnyElement {
    let width = if card.full_width {
        CARD_FULL_WIDTH
    } else {
        CARD_WIDTH
    };
    v_flex()
        .w(px(width))
        .max_w(relative(1.))
        .p(px(CARD_PADDING))
        .rounded(px(CARD_RADIUS))
        .bg(theme::surface())
        .border(px(CARD_BORDER_WIDTH))
        .border_color(theme::border())
        .text_color(theme::text_strong())
        .child(items_view(&card.items, id, cx))
        .when(!card.actions.is_empty(), |card_element| {
            card_element.child(
                div()
                    .mt(px(spacing_pixels(CardSpacing::Default)))
                    .child(actions_view(&card.actions, &format!("{id}-actions"))),
            )
        })
        .into_any_element()
}

fn items_view(items: &[CardItem], id: &str, cx: &App) -> Div {
    v_flex()
        .w_full()
        .children(items.iter().enumerate().map(|(index, item)| {
            let item_id = format!("{id}-{index}");
            div()
                .w_full()
                .when(index > 0, |wrapper| {
                    wrapper.mt(px(spacing_pixels(item.spacing)))
                })
                .when(index > 0 && item.separator, |wrapper| {
                    wrapper
                        .pt(px(SEPARATOR_PADDING))
                        .border_t_1()
                        .border_color(theme::border())
                })
                .child(element_view(&item.element, &item_id, cx))
        }))
}

fn spacing_pixels(spacing: CardSpacing) -> f32 {
    match spacing {
        CardSpacing::None => 0.,
        CardSpacing::Small => 4.,
        CardSpacing::Default => 8.,
        CardSpacing::Medium => 12.,
        CardSpacing::Large | CardSpacing::Padding => 16.,
        CardSpacing::ExtraLarge => 24.,
    }
}

fn element_view(element: &CardElement, id: &str, cx: &App) -> AnyElement {
    match element {
        CardElement::Text(text) => text_view(text, id, cx),
        CardElement::Image(image) => image_view(image, id),
        CardElement::ImageSet(images) => h_flex()
            .flex_wrap()
            .gap(px(IMAGE_SET_GAP))
            .children(
                images
                    .iter()
                    .enumerate()
                    .map(|(index, image)| image_view(image, &format!("{id}-{index}"))),
            )
            .into_any_element(),
        CardElement::Columns(columns) => columns_view(columns, id, cx),
        CardElement::Container { style, items } => {
            let tint = container_tint(*style);
            items_view(items, id, cx)
                .when_some(tint, |container, tint| {
                    container
                        .p(px(CONTAINER_PADDING))
                        .rounded(px(CONTAINER_RADIUS))
                        .bg(tint)
                })
                .into_any_element()
        }
        CardElement::Facts(facts) => facts_view(facts, id, cx),
        CardElement::Actions(actions) => actions_view(actions, id).into_any_element(),
    }
}

fn container_tint(style: ContainerStyle) -> Option<Hsla> {
    match style {
        ContainerStyle::Default => None,
        ContainerStyle::Emphasis => Some(theme::surface_raised()),
        ContainerStyle::Accent => Some(theme::accent().opacity(CONTAINER_TINT)),
        ContainerStyle::Good => Some(theme::green().opacity(CONTAINER_TINT)),
        ContainerStyle::Warning => Some(theme::amber().opacity(CONTAINER_TINT)),
        ContainerStyle::Attention => Some(theme::red().opacity(CONTAINER_TINT)),
    }
}

fn text_size(size: TextSize) -> f32 {
    match size {
        TextSize::Small => 11.5,
        TextSize::Default => 13.5,
        TextSize::Medium => 15.,
        TextSize::Large => 17.5,
        TextSize::ExtraLarge => 21.,
    }
}

fn text_color(text: &CardText) -> Hsla {
    match text.color {
        TextColor::Accent => theme::accent_text(),
        TextColor::Good => theme::green(),
        TextColor::Warning => theme::amber(),
        TextColor::Attention => theme::red_soft(),
        TextColor::Default | TextColor::Dark | TextColor::Light if text.subtle => {
            theme::text_muted()
        }
        TextColor::Default | TextColor::Dark | TextColor::Light => theme::text_strong(),
    }
}

fn text_view(text: &CardText, id: &str, cx: &App) -> AnyElement {
    div()
        .w_full()
        .text_size(px(text_size(text.size)))
        .line_height(relative(TEXT_LINE_HEIGHT))
        .text_color(text_color(text))
        .when(text.bold, |element| {
            element.font_weight(FontWeight::SEMIBOLD)
        })
        .when(!text.wrap, |element| element.whitespace_nowrap())
        .child(render_blocks(&layout_blocks(&text.spans), id, false, cx))
        .into_any_element()
}

fn facts_view(facts: &[CardFact], id: &str, cx: &App) -> AnyElement {
    v_flex()
        .w_full()
        .gap(px(FACT_GAP))
        .children(facts.iter().enumerate().map(|(index, fact)| {
            h_flex()
                .w_full()
                .items_start()
                .gap(px(COLUMN_GAP))
                .text_size(px(text_size(TextSize::Default)))
                .line_height(relative(TEXT_LINE_HEIGHT))
                .child(
                    div()
                        .w(px(FACT_TITLE_WIDTH))
                        .flex_none()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::text_muted())
                        .child(fact.title.clone()),
                )
                .child(div().flex_1().min_w(px(0.)).child(render_blocks(
                    &layout_blocks(&fact.value),
                    &format!("{id}-{index}"),
                    false,
                    cx,
                )))
        }))
        .into_any_element()
}

fn columns_view(columns: &[CardColumn], id: &str, cx: &App) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .items_stretch()
        .w_full()
        .gap(px(COLUMN_GAP))
        .children(columns.iter().enumerate().map(|(index, column)| {
            let content = items_view(&column.items, &format!("{id}-{index}"), cx);
            let aligned = v_flex()
                .h_full()
                .map(|aligned| match column.vertical_alignment {
                    VerticalAlignment::Top => aligned.justify_start(),
                    VerticalAlignment::Center => aligned.justify_center(),
                    VerticalAlignment::Bottom => aligned.justify_end(),
                });
            sized_column(aligned, column.width).child(content)
        }))
        .into_any_element()
}

fn sized_column(column: Div, width: ColumnWidth) -> Div {
    match width {
        ColumnWidth::Auto => column.flex_none(),
        ColumnWidth::Stretch => column.flex_1().min_w(px(0.)),
        ColumnWidth::Weighted(weight) => column
            .flex_basis(px(0.))
            .flex_grow(weight.max(0.))
            .min_w(px(0.)),
        ColumnWidth::Pixels(pixels) => column.w(px(pixels)).flex_none(),
    }
}

fn image_view(image: &CardImage, id: &str) -> AnyElement {
    let width = match (image.width, image.height, named_image_width(image.size)) {
        (Some(width), _, _) => width,
        (None, Some(height), _) => height,
        (None, None, Some(width)) => width,
        (None, None, None) => MEDIUM_IMAGE,
    }
    .min(CARD_FULL_WIDTH - 2. * CARD_PADDING);
    let height = image.height.unwrap_or(width);
    let radius = px(if image.person {
        width.min(height) / 2.
    } else {
        IMAGE_RADIUS
    });
    let frame = div()
        .id(ElementId::Name(id.to_owned().into()))
        .w(px(width))
        .h(px(height))
        .max_w(relative(1.))
        .flex_none()
        .overflow_hidden()
        .rounded(radius);
    if !image.url.starts_with(EXTERNAL_IMAGE_SCHEME) {
        return frame.bg(theme::surface_raised()).into_any_element();
    }
    frame
        .child(
            img(image.url.clone())
                .size_full()
                .rounded(radius)
                .object_fit(ObjectFit::Cover)
                .with_loading(move || image_placeholder(radius))
                .with_fallback(move || image_placeholder(radius)),
        )
        .into_any_element()
}

fn image_placeholder(radius: Pixels) -> AnyElement {
    div()
        .size_full()
        .rounded(radius)
        .bg(theme::surface_raised())
        .into_any_element()
}

fn named_image_width(size: ImageSize) -> Option<f32> {
    match size {
        ImageSize::Small => Some(SMALL_IMAGE),
        ImageSize::Medium => Some(MEDIUM_IMAGE),
        ImageSize::Large => Some(LARGE_IMAGE),
        ImageSize::Stretch => Some(CARD_WIDTH - 2. * CARD_PADDING),
        ImageSize::Auto => None,
    }
}

fn actions_view(actions: &[CardAction], id: &str) -> Div {
    h_flex().w_full().flex_wrap().gap(px(ACTION_GAP)).children(
        actions
            .iter()
            .enumerate()
            .map(|(index, action)| action_button(action, format!("{id}-{index}"))),
    )
}

fn action_button(action: &CardAction, id: String) -> Stateful<Div> {
    let button = div()
        .id(ElementId::Name(id.into()))
        .px(px(12.))
        .py(px(6.))
        .rounded(px(ACTION_RADIUS))
        .border_1()
        .border_color(theme::border_strong())
        .text_size(px(ACTION_TEXT_SIZE))
        .font_weight(FontWeight::SEMIBOLD)
        .child(action.title.clone());
    match &action.kind {
        CardActionKind::OpenUrl(url) => {
            let url = url.clone();
            button
                .text_color(theme::accent_text())
                .cursor_pointer()
                .hover(|button| button.bg(theme::row_hover()))
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    cx.open_url(&url);
                })
        }
        CardActionKind::Unsupported => button
            .text_color(theme::text_muted())
            .opacity(ACTION_DISABLED_OPACITY)
            .tooltip(|window, cx| Tooltip::new(ACTION_DISABLED_TOOLTIP).build(window, cx))
            .on_click(|_, _, cx| cx.stop_propagation()),
    }
}
