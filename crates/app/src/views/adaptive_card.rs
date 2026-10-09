use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::component::{ActiveTheme as _, h_flex, tooltip::Tooltip, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::{
    AdaptiveCard, CardAction, CardActionKind, CardColumn, CardElement, CardFact, CardImage,
    CardInput, CardItem, CardSpacing, CardText, ColumnWidth, ContainerLayout, ContainerStyle,
    HorizontalAlignment, ImageSize, TextColor, TextSize, VerticalAlignment, WidthClass,
    split_overflow,
};

use super::card_carousel::carousel_view;
use super::card_chart::chart_view;
use super::card_code_block::code_block_view;
use super::card_icon::{action_icon_view, icon_view};
use super::card_input::{input_view, rating_display_view};
use super::card_media::media_view;
use super::card_table::table_view;
use super::card_widgets::{
    badge_view, compound_button_view, leading, progress_bar_view, progress_ring_view,
};
use super::widgets::symbol;
use crate::app_state::{AppHandle, AppState};
use crate::card_state::{ActionPhase, CardScope, CardState};
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
const ACTION_BUSY_OPACITY: f32 = 0.6;
const ACTION_CHECK_SIZE: f32 = 14.;
const ACTION_ICON_GAP: f32 = 4.;
const NOTE_TEXT_SIZE: f32 = 12.;
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
const EXTERNAL_IMAGE_SCHEMES: [&str; 2] = ["https://", "http://"];
const ACTION_OVERFLOW_LABEL: &str = "...";
const ACTION_OVERFLOW_TOOLTIP: &str = "More actions";
const OVERFLOW_PANEL_PADDING: f32 = 4.;
const SELECT_TINT: f32 = 0.07;

pub(super) type ClickHandler = Rc<dyn Fn(&mut App)>;

pub(super) struct CardContext<'a> {
    pub(super) cx: &'a App,
    pub(super) scope: &'a CardScope,
    pub(super) state: &'a CardState,
    root_key: String,
    initial_visibility: Rc<HashMap<String, bool>>,
    inputs: Vec<CardInput>,
    width_class: WidthClass,
}

impl<'a> CardContext<'a> {
    fn new(card: &AdaptiveCard, scope: &'a CardScope, cx: &'a App) -> Self {
        CardContext {
            cx,
            scope,
            state: &cx.global::<AppHandle>().0.read(cx).cards,
            root_key: scope.card_key(),
            initial_visibility: Rc::new(card.element_visibility()),
            inputs: card.own_inputs(),
            width_class: WidthClass::from_pixels(card_width(card)),
        }
    }

    fn nested(&self, card: &AdaptiveCard) -> CardContext<'a> {
        let mut inputs = self.inputs.clone();
        inputs.extend(card.own_inputs());
        CardContext {
            cx: self.cx,
            scope: self.scope,
            state: self.state,
            root_key: self.root_key.clone(),
            initial_visibility: self.initial_visibility.clone(),
            inputs,
            width_class: self.width_class,
        }
    }

    pub(super) fn visible(&self, id: Option<&String>, initial: bool) -> bool {
        match id {
            Some(id) => self
                .state
                .is_visible(&format!("{}/{id}", self.root_key), initial),
            None => initial,
        }
    }
}

pub fn cards_view(
    cards: &[AdaptiveCard],
    conversation_id: &str,
    message_id: &str,
    cx: &App,
) -> Vec<AnyElement> {
    cards
        .iter()
        .enumerate()
        .map(|(index, card)| {
            let scope = CardScope::message(conversation_id, message_id, index);
            card_view(card, &scope, cx)
        })
        .collect()
}

pub fn ensure_cards_inputs(
    app: &Entity<AppState>,
    cards: &[AdaptiveCard],
    conversation_id: &str,
    message_id: &str,
    window: &mut Window,
    cx: &mut App,
) {
    if cards.is_empty() {
        return;
    }
    app.update(cx, |state, cx| {
        for (index, card) in cards.iter().enumerate() {
            let scope = CardScope::message(conversation_id, message_id, index);
            state.ensure_card_inputs(&scope, card, window, cx);
        }
    });
}

pub fn request_card_refreshes(
    app: &Entity<AppState>,
    cards: &[AdaptiveCard],
    conversation_id: &str,
    message_id: &str,
    cx: &mut App,
) {
    if cards.iter().all(|card| card.refresh.is_none()) {
        return;
    }
    app.update(cx, |state, cx| {
        state.request_card_refreshes(conversation_id, message_id, cards, cx)
    });
}

pub fn card_view(card: &AdaptiveCard, scope: &CardScope, cx: &App) -> AnyElement {
    let context = CardContext::new(card, scope, cx);
    let id = scope.card_key();
    let note = context.state.note(&id).map(str::to_owned);
    let select_handler = card
        .select_action
        .as_ref()
        .filter(|action| action.is_clickable())
        .and_then(|action| click_handler(action, &format!("{id}-select"), None, &context));
    let visible_actions = card
        .actions
        .iter()
        .any(|action| context.visible(action.id.as_ref(), action.visible));
    v_flex()
        .id(ElementId::Name(format!("{id}-root").into()))
        .relative()
        .w(px(card_width(card)))
        .max_w(relative(1.))
        .p(px(CARD_PADDING))
        .rounded(px(CARD_RADIUS))
        .bg(theme::surface())
        .border(px(CARD_BORDER_WIDTH))
        .border_color(theme::border())
        .text_color(theme::text_strong())
        .when_some(select_handler, |root, handler| {
            root.cursor_pointer()
                .hover(|root| root.bg(theme::row_hover()))
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    handler(cx)
                })
        })
        .when_some(card.layout.background_image.clone(), |root, url| {
            root.child(background_layer(&url, CARD_RADIUS))
        })
        .child(aligned_body(&card.items, &id, &context, &card.layout, true))
        .when(visible_actions, |card_element| {
            card_element.child(div().mt(px(spacing_pixels(CardSpacing::Default))).child(
                actions_view(&card.actions, &format!("{id}-actions"), &context),
            ))
        })
        .when_some(note, |card_element, note| {
            card_element.child(
                div()
                    .mt(px(spacing_pixels(CardSpacing::Default)))
                    .text_size(px(NOTE_TEXT_SIZE))
                    .text_color(theme::text_muted())
                    .child(note),
            )
        })
        .into_any_element()
}

fn card_width(card: &AdaptiveCard) -> f32 {
    if card.full_width {
        CARD_FULL_WIDTH
    } else {
        CARD_WIDTH
    }
}

fn background_layer(url: &str, radius: f32) -> AnyElement {
    div()
        .absolute()
        .inset_0()
        .rounded(px(radius))
        .overflow_hidden()
        .child(
            img(url.to_owned())
                .size_full()
                .object_fit(ObjectFit::Cover)
                .with_loading(|| div().into_any_element())
                .with_fallback(|| div().into_any_element()),
        )
        .into_any_element()
}

fn aligned_body(
    items: &[CardItem],
    id: &str,
    context: &CardContext,
    layout: &ContainerLayout,
    top_level: bool,
) -> Div {
    items_view(items, id, context, top_level)
        .when_some(layout.min_height, |body, height| body.min_h(px(height)))
        .map(|body| match layout.vertical_alignment {
            VerticalAlignment::Top => body.justify_start(),
            VerticalAlignment::Center => body.justify_center(),
            VerticalAlignment::Bottom => body.justify_end(),
        })
        .when(layout.rtl, |body| body.text_right())
}

pub(super) fn items_view(
    items: &[CardItem],
    id: &str,
    context: &CardContext,
    top_level: bool,
) -> Div {
    let shown: Vec<(usize, &CardItem)> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            context.visible(item.id.as_ref(), item.visible)
                && item
                    .target_width
                    .is_none_or(|target| target.matches(context.width_class))
        })
        .collect();
    let mut column = v_flex().w_full();
    for (position, (index, item)) in shown.iter().enumerate() {
        let item_id = format!("{id}-{index}");
        let first = position == 0;
        let bleed = top_level
            .then(|| bleed_edges(item, first, position + 1 == shown.len()))
            .flatten();
        column = column.child(
            div()
                .map(|wrapper| match bleed {
                    Some(edges) => wrapper
                        .mx(px(-CARD_PADDING))
                        .when(edges.top, |wrapper| wrapper.mt(px(-CARD_PADDING)))
                        .when(edges.bottom, |wrapper| wrapper.mb(px(-CARD_PADDING))),
                    None => wrapper.w_full(),
                })
                .when(item.stretch, |wrapper| wrapper.flex_grow(1.))
                .when(!first && bleed.is_none_or(|edges| !edges.top), |wrapper| {
                    wrapper.mt(px(spacing_pixels(item.spacing)))
                })
                .when(!first && item.separator, |wrapper| {
                    wrapper
                        .pt(px(SEPARATOR_PADDING))
                        .border_t_1()
                        .border_color(theme::border())
                })
                .child(element_view(&item.element, &item_id, context, bleed)),
        );
    }
    column
}

#[derive(Clone, Copy)]
pub(super) struct BleedEdges {
    top: bool,
    bottom: bool,
}

fn bleed_edges(item: &CardItem, first: bool, last: bool) -> Option<BleedEdges> {
    match &item.element {
        CardElement::Container { layout, .. } if layout.bleed => Some(BleedEdges {
            top: first,
            bottom: last,
        }),
        _ => None,
    }
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

fn element_view(
    element: &CardElement,
    id: &str,
    context: &CardContext,
    bleed: Option<BleedEdges>,
) -> AnyElement {
    match element {
        CardElement::Text(text) => text_view(text, id, context.cx),
        CardElement::Image(image) => aligned_image(image, id, context),
        CardElement::ImageSet(images) => {
            h_flex()
                .flex_wrap()
                .gap(px(IMAGE_SET_GAP))
                .children(images.iter().enumerate().map(|(index, image)| {
                    selectable_image(image, &format!("{id}-{index}"), context)
                }))
                .into_any_element()
        }
        CardElement::Columns { columns, layout } => columns_view(columns, layout, id, context),
        CardElement::Container {
            layout,
            items,
            select_action,
        } => selectable(
            container_view(items, id, context, layout, bleed).into_any_element(),
            select_action.as_ref(),
            id,
            false,
            context,
        ),
        CardElement::Facts(facts) => facts_view(facts, id, context.cx),
        CardElement::Media(media) => media_view(media, id),
        CardElement::Icon(icon) => selectable(
            icon_view(icon),
            icon.select_action.as_ref(),
            id,
            true,
            context,
        ),
        CardElement::Table(table) => table_view(table, id, context),
        CardElement::CodeBlock(block) => code_block_view(block, id, context),
        CardElement::Badge(badge) => leading(badge_view(badge, id)),
        CardElement::ProgressBar(bar) => progress_bar_view(bar, id),
        CardElement::ProgressRing(ring) => leading(progress_ring_view(ring, id)),
        CardElement::CompoundButton(button) => compound_button_view(button, id, context),
        CardElement::Carousel(carousel) => carousel_view(carousel, id, context),
        CardElement::Actions(actions) => actions_view(actions, id, context).into_any_element(),
        CardElement::Input(input) => {
            input_view(input, context.scope, context.state, &context.inputs)
        }
        CardElement::Rating(display) => rating_display_view(display),
        CardElement::Chart(chart) => chart_view(chart),
    }
}

pub(super) fn container_view(
    items: &[CardItem],
    id: &str,
    context: &CardContext,
    layout: &ContainerLayout,
    bleed: Option<BleedEdges>,
) -> Div {
    let tint = container_tint(layout.style);
    let padded = tint.is_some() || layout.background_image.is_some();
    let inset = |edge: bool| {
        if edge {
            CARD_PADDING
        } else if padded {
            CONTAINER_PADDING
        } else {
            0.
        }
    };
    v_flex()
        .w_full()
        .relative()
        .map(|container| match bleed {
            Some(edges) => container
                .px(px(CARD_PADDING))
                .pt(px(inset(edges.top)))
                .pb(px(inset(edges.bottom)))
                .when(edges.top, |container| {
                    container.rounded_t(px(CARD_RADIUS - CARD_BORDER_WIDTH))
                })
                .when(edges.bottom, |container| {
                    container.rounded_b(px(CARD_RADIUS - CARD_BORDER_WIDTH))
                }),
            None => container.when(padded, |container| {
                container
                    .p(px(CONTAINER_PADDING))
                    .rounded(px(CONTAINER_RADIUS))
            }),
        })
        .when_some(tint, |container, tint| container.bg(tint))
        .when_some(layout.background_image.clone(), |container, url| {
            container.child(background_layer(&url, CONTAINER_RADIUS))
        })
        .child(aligned_body(items, id, context, layout, false))
}

pub(super) fn container_tint(style: ContainerStyle) -> Option<Hsla> {
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
    card_color(text.color, text.subtle)
}

pub(super) fn card_color(color: TextColor, subtle: bool) -> Hsla {
    match color {
        TextColor::Accent => theme::accent_text(),
        TextColor::Good => theme::green(),
        TextColor::Warning => theme::amber(),
        TextColor::Attention => theme::red_soft(),
        TextColor::Default | TextColor::Dark | TextColor::Light if subtle => theme::text_muted(),
        TextColor::Default | TextColor::Dark | TextColor::Light => theme::text_strong(),
    }
}

fn text_view(text: &CardText, id: &str, cx: &App) -> AnyElement {
    div()
        .w_full()
        .min_w(px(0.))
        .text_size(px(text_size(text.size)))
        .line_height(relative(TEXT_LINE_HEIGHT))
        .text_color(text_color(text))
        .when(text.bold, |element| {
            element.font_weight(FontWeight::SEMIBOLD)
        })
        .when(text.monospace, |element| {
            element.font_family(cx.theme().mono_font_family.clone())
        })
        .map(|element| match text.alignment {
            Some(HorizontalAlignment::Left) => element.text_left(),
            Some(HorizontalAlignment::Center) => element.text_center(),
            Some(HorizontalAlignment::Right) => element.text_right(),
            None => element,
        })
        .map(|element| match (text.wrap, text.max_lines) {
            (false, _) => element
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis(),
            (true, Some(lines)) => element.text_ellipsis().line_clamp(lines),
            (true, None) => element,
        })
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

fn columns_view(
    columns: &[CardColumn],
    layout: &ContainerLayout,
    id: &str,
    context: &CardContext,
) -> AnyElement {
    let tint = container_tint(layout.style);
    div()
        .flex()
        .map(|row| {
            if layout.rtl {
                row.flex_row_reverse()
            } else {
                row.flex_row()
            }
        })
        .items_stretch()
        .w_full()
        .gap(px(COLUMN_GAP))
        .when_some(layout.min_height, |row, height| row.min_h(px(height)))
        .when_some(tint, |row, tint| {
            row.p(px(CONTAINER_PADDING))
                .rounded(px(CONTAINER_RADIUS))
                .bg(tint)
        })
        .children(
            columns
                .iter()
                .enumerate()
                .filter(|(_, column)| {
                    column
                        .target_width
                        .is_none_or(|target| target.matches(context.width_class))
                })
                .map(|(index, column)| {
                    let content = container_view(
                        &column.items,
                        &format!("{id}-{index}"),
                        context,
                        &column.layout,
                        None,
                    );
                    let content = selectable(
                        content.into_any_element(),
                        column.select_action.as_ref(),
                        &format!("{id}-{index}"),
                        false,
                        context,
                    );
                    let aligned =
                        v_flex()
                            .h_full()
                            .map(|aligned| match column.layout.vertical_alignment {
                                VerticalAlignment::Top => aligned.justify_start(),
                                VerticalAlignment::Center => aligned.justify_center(),
                                VerticalAlignment::Bottom => aligned.justify_end(),
                            });
                    sized_column(aligned, column.width).child(content)
                }),
        )
        .into_any_element()
}

pub(super) fn sized_column(column: Div, width: ColumnWidth) -> Div {
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

fn aligned_image(image: &CardImage, id: &str, context: &CardContext) -> AnyElement {
    h_flex()
        .w_full()
        .map(|row| match image.alignment {
            HorizontalAlignment::Left => row.justify_start(),
            HorizontalAlignment::Center => row.justify_center(),
            HorizontalAlignment::Right => row.justify_end(),
        })
        .child(selectable_image(image, id, context))
        .into_any_element()
}

fn selectable_image(image: &CardImage, id: &str, context: &CardContext) -> AnyElement {
    selectable(
        image_view(image, id),
        image.select_action.as_ref(),
        id,
        true,
        context,
    )
}

pub(super) fn selectable(
    content: AnyElement,
    select_action: Option<&CardAction>,
    id: &str,
    fit: bool,
    context: &CardContext,
) -> AnyElement {
    let select_key = format!("{id}-select");
    let Some(handler) = select_action
        .filter(|action| action.is_clickable())
        .and_then(|action| click_handler(action, &select_key, None, context))
    else {
        return content;
    };
    let group = SharedString::from(select_key.clone());
    div()
        .id(ElementId::Name(select_key.into()))
        .group(group.clone())
        .relative()
        .cursor_pointer()
        .map(|wrapper| {
            if fit {
                wrapper.self_start().flex_none()
            } else {
                wrapper.w_full()
            }
        })
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            handler(cx)
        })
        .child(content)
        .child(
            div()
                .absolute()
                .inset_0()
                .rounded(px(CONTAINER_RADIUS))
                .group_hover(group, |tint| tint.bg(white().opacity(SELECT_TINT))),
        )
        .into_any_element()
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
    if !EXTERNAL_IMAGE_SCHEMES
        .iter()
        .any(|scheme| image.url.starts_with(scheme))
    {
        return frame.bg(theme::surface_raised()).into_any_element();
    }
    let alt_text = image.alt_text.clone();
    frame
        .when_some(image.background_color, |frame, color| frame.bg(rgba(color)))
        .when_some(alt_text, |frame, alt_text| {
            frame.tooltip(move |window, cx| Tooltip::new(alt_text.clone()).build(window, cx))
        })
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

fn actions_view(actions: &[CardAction], id: &str, context: &CardContext) -> Div {
    let open_card = context
        .state
        .open_card(id)
        .and_then(|index| actions.get(index))
        .and_then(|action| match &action.kind {
            CardActionKind::ShowCard(card) => Some(card),
            _ => None,
        });
    let split = split_overflow(actions, |action| {
        context.visible(action.id.as_ref(), action.visible)
    });
    let overflow_key = format!("{id}-overflow");
    let overflow_open = !split.overflow.is_empty() && context.state.slot(&overflow_key, 0) == 1;
    v_flex()
        .w_full()
        .gap(px(ACTION_GAP))
        .child(
            h_flex()
                .w_full()
                .flex_wrap()
                .gap(px(ACTION_GAP))
                .children(
                    split
                        .primary
                        .iter()
                        .map(|&index| action_button(&actions[index], index, id, context)),
                )
                .when(!split.overflow.is_empty(), |row| {
                    row.child(overflow_button(&overflow_key, overflow_open, context))
                }),
        )
        .when(overflow_open, |column| {
            column.child(
                v_flex()
                    .self_start()
                    .items_start()
                    .gap(px(OVERFLOW_PANEL_PADDING))
                    .p(px(OVERFLOW_PANEL_PADDING))
                    .rounded(px(CONTAINER_RADIUS))
                    .bg(theme::surface_raised())
                    .children(
                        split
                            .overflow
                            .iter()
                            .map(|&index| action_button(&actions[index], index, id, context)),
                    ),
            )
        })
        .when_some(open_card, |column, card| {
            let nested_id = format!("{id}-show");
            let nested_context = context.nested(card);
            column.child(
                v_flex()
                    .w_full()
                    .p(px(CONTAINER_PADDING))
                    .gap(px(ACTION_GAP))
                    .rounded(px(CONTAINER_RADIUS))
                    .bg(theme::surface_raised())
                    .child(items_view(&card.items, &nested_id, &nested_context, false))
                    .when(!card.actions.is_empty(), |nested| {
                        nested.child(actions_view(
                            &card.actions,
                            &format!("{nested_id}-actions"),
                            &nested_context,
                        ))
                    }),
            )
        })
}

fn overflow_button(overflow_key: &str, open: bool, context: &CardContext) -> Stateful<Div> {
    let handler = slot_handler(context, overflow_key, usize::from(!open));
    action_frame(overflow_key, theme::border_strong())
        .text_color(theme::accent_text())
        .cursor_pointer()
        .hover(|button| button.bg(theme::row_hover()))
        .when(open, |button| button.bg(theme::row_hover()))
        .tooltip(|window, cx| Tooltip::new(ACTION_OVERFLOW_TOOLTIP).build(window, cx))
        .child(ACTION_OVERFLOW_LABEL)
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            handler(cx)
        })
}

fn action_frame(button_id: &str, border: Hsla) -> Stateful<Div> {
    div()
        .id(ElementId::Name(button_id.to_owned().into()))
        .px(px(12.))
        .py(px(6.))
        .rounded(px(ACTION_RADIUS))
        .border_1()
        .border_color(border)
        .text_size(px(ACTION_TEXT_SIZE))
        .font_weight(FontWeight::SEMIBOLD)
}

pub(super) fn slot_handler(context: &CardContext, slot_key: &str, value: usize) -> ClickHandler {
    let scope = context.scope.clone();
    let slot_key = slot_key.to_owned();
    Rc::new(move |cx| {
        let (scope, slot_key) = (scope.clone(), slot_key.clone());
        cx.global::<AppHandle>().0.clone().update(cx, |state, cx| {
            state.set_card_slot(&scope, &slot_key, value, cx)
        });
    })
}

fn action_button(
    action: &CardAction,
    index: usize,
    actions_id: &str,
    context: &CardContext,
) -> Stateful<Div> {
    let button_id = format!("{actions_id}-{index}");
    let phase = context.state.phase(&button_id).cloned();
    let label = action_label(
        action,
        &phase,
        context.state.open_card(actions_id) == Some(index),
    );
    let border = match phase {
        Some(ActionPhase::Failed(_)) => theme::red(),
        _ => theme::border_strong(),
    };
    let button = action_frame(&button_id, border).child(label);
    if !action.is_clickable() {
        let hint = match action.kind {
            CardActionKind::Unavailable(reason) if action.enabled => Some(reason.to_owned()),
            _ if action.enabled => Some(ACTION_DISABLED_TOOLTIP.to_owned()),
            _ => action.tooltip.clone(),
        };
        return button
            .text_color(theme::text_muted())
            .opacity(ACTION_DISABLED_OPACITY)
            .when_some(hint, |button, hint| {
                button.tooltip(move |window, cx| Tooltip::new(hint.clone()).build(window, cx))
            })
            .on_click(|_, _, cx| cx.stop_propagation());
    }
    let enabled = button.text_color(theme::accent_text());
    if phase == Some(ActionPhase::Busy) {
        return enabled
            .opacity(ACTION_BUSY_OPACITY)
            .on_click(|_, _, cx| cx.stop_propagation());
    }
    let hint = failure_reason(&phase).or_else(|| action.tooltip.clone());
    let enabled = enabled
        .cursor_pointer()
        .hover(|button| button.bg(theme::row_hover()))
        .when_some(hint, |button, hint| {
            button.tooltip(move |window, cx| Tooltip::new(hint.clone()).build(window, cx))
        });
    match click_handler(action, &button_id, Some((actions_id, index)), context) {
        Some(handler) => enabled.on_click(move |_, _, cx| {
            cx.stop_propagation();
            handler(cx)
        }),
        None => enabled,
    }
}

fn click_handler(
    action: &CardAction,
    action_key: &str,
    show_card: Option<(&str, usize)>,
    context: &CardContext,
) -> Option<ClickHandler> {
    let scope = context.scope.clone();
    match &action.kind {
        CardActionKind::OpenUrl(url) => {
            let url = url.clone();
            Some(Rc::new(move |cx| cx.open_url(&url)))
        }
        CardActionKind::Submit(_) | CardActionKind::Execute(_) => {
            let action = action.clone();
            let action_key = action_key.to_owned();
            let inputs = context.inputs.clone();
            Some(Rc::new(move |cx| {
                let (scope, action, action_key, inputs) = (
                    scope.clone(),
                    action.clone(),
                    action_key.clone(),
                    inputs.clone(),
                );
                cx.global::<AppHandle>().0.clone().update(cx, |state, cx| {
                    state.run_card_action(scope, action_key, action, inputs, cx)
                });
            }))
        }
        CardActionKind::ShowCard(_) => {
            let (actions_id, index) = show_card?;
            let actions_id = actions_id.to_owned();
            Some(Rc::new(move |cx| {
                let (scope, actions_id) = (scope.clone(), actions_id.clone());
                cx.global::<AppHandle>().0.clone().update(cx, |state, cx| {
                    state.toggle_show_card(&scope, &actions_id, index, cx)
                });
            }))
        }
        CardActionKind::ToggleVisibility(targets) => {
            let elements: Vec<(String, bool, Option<bool>)> = targets
                .iter()
                .map(|target| {
                    let initial = context
                        .initial_visibility
                        .get(&target.element_id)
                        .copied()
                        .unwrap_or(true);
                    (target.element_id.clone(), initial, target.visible)
                })
                .collect();
            let root_key = context.root_key.clone();
            Some(Rc::new(move |cx| {
                let (scope, root_key, elements) =
                    (scope.clone(), root_key.clone(), elements.clone());
                cx.global::<AppHandle>().0.clone().update(cx, |state, cx| {
                    state.toggle_card_elements(&scope, &root_key, elements, cx)
                });
            }))
        }
        CardActionKind::Unavailable(_) | CardActionKind::Unsupported => None,
    }
}

fn failure_reason(phase: &Option<ActionPhase>) -> Option<String> {
    match phase {
        Some(ActionPhase::Failed(reason)) => Some(reason.clone()),
        _ => None,
    }
}

fn action_label(action: &CardAction, phase: &Option<ActionPhase>, open: bool) -> AnyElement {
    match phase {
        Some(ActionPhase::Busy) => format!("{}...", action.title).into_any_element(),
        Some(ActionPhase::Done) => h_flex()
            .gap(px(ACTION_ICON_GAP))
            .items_center()
            .child(symbol("done", ACTION_CHECK_SIZE, theme::green()))
            .child(action.title.clone())
            .into_any_element(),
        _ if open => titled(action, format!("{} ^", action.title)),
        _ => titled(action, action.title.clone()),
    }
}

fn titled(action: &CardAction, text: String) -> AnyElement {
    match action.icon.as_ref().map(action_icon_view) {
        Some(icon) => h_flex()
            .gap(px(ACTION_ICON_GAP))
            .items_center()
            .child(icon)
            .child(text)
            .into_any_element(),
        None => text.into_any_element(),
    }
}
