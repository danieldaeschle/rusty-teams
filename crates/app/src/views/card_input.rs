use gpui_kit::assets::IconName;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::date_picker::DatePicker;
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::radio::{Radio, RadioGroup};
use gpui_kit::component::select::Select;
use gpui_kit::component::time_field::TimeField;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::{
    CardActionKind, CardInput, CardInputKind, ChoiceInput, DATE_PLACEHOLDER, InlineAction,
    RatingColor, RatingDisplay, RatingInput, RatingSize, RatingStyle, ToggleInput,
};

use super::widgets::{icon, symbol};
use crate::app_state::AppHandle;
use crate::card_inputs::{InputField, inline_action_key, uses_select};
use crate::card_state::{ActionPhase, CardScope, CardState};
use crate::theme;

const INPUT_GAP: f32 = 4.;
const LABEL_SIZE: f32 = 12.5;
const ERROR_SIZE: f32 = 12.;
const FIELD_RADIUS: f32 = 6.;
const CHOICE_GAP: f32 = 6.;
const STAR_GAP: f32 = 2.;
const STAR_MEDIUM: f32 = 20.;
const STAR_LARGE: f32 = 28.;
const RATING_TEXT_SIZE: f32 = 13.;
const RATING_TEXT_GAP: f32 = 6.;
const HALF: f64 = 0.5;
const INLINE_ICON_SIZE: f32 = 16.;
const INLINE_BUTTON_PADDING: f32 = 4.;
const INLINE_BUSY_OPACITY: f32 = 0.5;

pub fn input_view(
    input: &CardInput,
    scope: &CardScope,
    state: &CardState,
    inputs: &[CardInput],
) -> AnyElement {
    let key = scope.input_key(&input.id);
    let error = state.input_error(&key).map(str::to_owned);
    let label = input
        .label
        .as_deref()
        .filter(|label| !label.trim().is_empty());
    v_flex()
        .id(ElementId::Name(format!("{key}-input").into()))
        .w_full()
        .gap(px(INPUT_GAP))
        .on_click(|_, _, cx| cx.stop_propagation())
        .when_some(label, |column, label| {
            column.child(
                div()
                    .text_size(px(LABEL_SIZE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text_strong())
                    .child(if input.required {
                        format!("{label} *")
                    } else {
                        label.to_owned()
                    }),
            )
        })
        .child(
            div()
                .w_full()
                .rounded(px(FIELD_RADIUS))
                .border_1()
                .border_color(if error.is_some() {
                    theme::red()
                } else {
                    transparent_black()
                })
                .child(control_view(input, &key, scope, state, inputs)),
        )
        .when_some(error, |column, error| {
            column.child(
                div()
                    .text_size(px(ERROR_SIZE))
                    .text_color(theme::red_soft())
                    .child(error),
            )
        })
        .into_any_element()
}

fn control_view(
    input: &CardInput,
    key: &str,
    scope: &CardScope,
    state: &CardState,
    inputs: &[CardInput],
) -> AnyElement {
    let current = state.input_value(key).to_owned();
    match &input.kind {
        CardInputKind::Toggle(toggle) => toggle_view(toggle, key, &current, scope),
        CardInputKind::Rating(rating) => rating_input_view(rating, key, &current, scope),
        CardInputKind::Choice(choice) if !uses_select(choice) => {
            choices_view(choice, key, &current, scope)
        }
        _ => match state.input_field(key) {
            Some(InputField::Line(field)) => {
                let inline = match &input.kind {
                    CardInputKind::Text(text) => text.inline_action.as_ref(),
                    _ => None,
                };
                Input::new(field)
                    .when_some(inline, |line, inline| {
                        line.suffix(inline_action_view(inline, key, scope, state, inputs))
                    })
                    .into_any_element()
            }
            Some(InputField::Area(field)) => Textarea::new(field).into_any_element(),
            Some(InputField::Date(field)) => DatePicker::new(field)
                .placeholder(DATE_PLACEHOLDER)
                .cleanable(!input.required)
                .into_any_element(),
            Some(InputField::Time(field)) => TimeField::new(field)
                .invalid(state.input_error(key).is_some())
                .into_any_element(),
            Some(InputField::Select(field)) => {
                let placeholder = match &input.kind {
                    CardInputKind::Choice(choice) => choice.placeholder.clone(),
                    _ => None,
                };
                Select::new(field)
                    .w_full()
                    .when_some(placeholder, |select, placeholder| {
                        select.placeholder(placeholder)
                    })
                    .into_any_element()
            }
            None => div().into_any_element(),
        },
    }
}

fn inline_action_view(
    inline: &InlineAction,
    key: &str,
    scope: &CardScope,
    state: &CardState,
    inputs: &[CardInput],
) -> AnyElement {
    let action_key = inline_action_key(key);
    let phase = state.phase(&action_key).cloned();
    let glyph = match (&phase, &inline.icon_url) {
        (Some(ActionPhase::Done), _) => {
            symbol("done", INLINE_ICON_SIZE, theme::green()).into_any_element()
        }
        (_, Some(url)) if url.starts_with("https://") => img(url.clone())
            .size(px(INLINE_ICON_SIZE))
            .into_any_element(),
        _ => {
            let name = match inline.action.kind {
                CardActionKind::OpenUrl(_) => IconName::ExternalLink,
                _ => IconName::SendHorizontal,
            };
            icon(name, INLINE_ICON_SIZE, theme::accent_text()).into_any_element()
        }
    };
    let (scope, action, inputs) = (scope.clone(), inline.action.clone(), inputs.to_vec());
    div()
        .id(ElementId::Name(action_key.clone().into()))
        .p(px(INLINE_BUTTON_PADDING))
        .rounded(px(FIELD_RADIUS))
        .cursor_pointer()
        .hover(|button| button.bg(theme::row_hover()))
        .when(phase == Some(ActionPhase::Busy), |button| {
            button.opacity(INLINE_BUSY_OPACITY)
        })
        .child(glyph)
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            let (scope, action, action_key, inputs) = (
                scope.clone(),
                action.clone(),
                action_key.clone(),
                inputs.clone(),
            );
            cx.global::<AppHandle>().0.clone().update(cx, |state, cx| {
                state.run_inline_action(&scope, action_key, &action, &inputs, cx)
            });
        })
        .into_any_element()
}

fn star_pixels(size: RatingSize) -> f32 {
    match size {
        RatingSize::Medium => STAR_MEDIUM,
        RatingSize::Large => STAR_LARGE,
    }
}

fn star_color(color: RatingColor) -> Hsla {
    match color {
        RatingColor::Neutral => theme::text_strong(),
        RatingColor::Marigold => theme::amber(),
    }
}

fn star_icon(rating: f64, position: u32, pixels: f32, color: Hsla) -> AnyElement {
    let position = f64::from(position);
    if rating >= position {
        icon(IconName::StarFill, pixels, color).into_any_element()
    } else if rating >= position - HALF {
        div()
            .relative()
            .size(px(pixels))
            .child(icon(IconName::Star, pixels, theme::text_muted()))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w(px(pixels / 2.))
                    .h(px(pixels))
                    .overflow_hidden()
                    .child(icon(IconName::StarFill, pixels, color)),
            )
            .into_any_element()
    } else {
        icon(IconName::Star, pixels, theme::text_muted()).into_any_element()
    }
}

fn rating_input_view(
    rating: &RatingInput,
    key: &str,
    current: &str,
    scope: &CardScope,
) -> AnyElement {
    let value = rating.rating(current);
    let pixels = star_pixels(rating.size);
    let color = star_color(rating.color);
    h_flex()
        .gap(px(STAR_GAP))
        .children((1..=rating.max).map(|position| {
            let zones: Vec<f64> = if rating.allow_half {
                vec![f64::from(position) - HALF, f64::from(position)]
            } else {
                vec![f64::from(position)]
            };
            div()
                .relative()
                .flex_none()
                .size(px(pixels))
                .child(star_icon(value, position, pixels, color))
                .child(
                    h_flex()
                        .absolute()
                        .inset_0()
                        .children(
                            zones
                                .into_iter()
                                .enumerate()
                                .map(|(zone_index, zone_value)| {
                                    let (scope, input_key, rating) =
                                        (scope.clone(), key.to_owned(), rating.clone());
                                    div()
                                        .id(ElementId::Name(
                                            format!("{key}-star-{position}-{zone_index}").into(),
                                        ))
                                        .flex_1()
                                        .h_full()
                                        .cursor_pointer()
                                        .on_click(move |_, _, cx| {
                                            cx.stop_propagation();
                                            pick(
                                                &scope,
                                                &input_key,
                                                rating.value_text(zone_value),
                                                cx,
                                            );
                                        })
                                }),
                        ),
                )
        }))
        .into_any_element()
}

pub fn rating_display_view(display: &RatingDisplay) -> AnyElement {
    let pixels = star_pixels(display.size);
    let color = star_color(RatingColor::Marigold);
    let count = display.count.map(|count| format!("({count})"));
    let row = match display.style {
        RatingStyle::Compact => h_flex()
            .gap(px(RATING_TEXT_GAP))
            .child(star_icon(display.value, 1, pixels, color))
            .child(format!("{:.1}", display.value)),
        RatingStyle::Default => h_flex().gap(px(STAR_GAP)).children(
            (1..=display.max).map(|position| star_icon(display.value, position, pixels, color)),
        ),
    };
    row.items_center()
        .text_size(px(RATING_TEXT_SIZE))
        .text_color(theme::text_strong())
        .when_some(count, |row, count| {
            row.child(
                div()
                    .ml(px(RATING_TEXT_GAP - STAR_GAP))
                    .text_color(theme::text_muted())
                    .child(count),
            )
        })
        .into_any_element()
}

fn toggle_view(toggle: &ToggleInput, key: &str, current: &str, scope: &CardScope) -> AnyElement {
    let (value_on, value_off) = (toggle.value_on.clone(), toggle.value_off.clone());
    let (scope, input_key) = (scope.clone(), key.to_owned());
    Checkbox::new(ElementId::Name(format!("{key}-toggle").into()))
        .label(toggle.title.clone())
        .checked(toggle.is_on(current))
        .when(!toggle.wrap, |checkbox| {
            checkbox.whitespace_nowrap().text_ellipsis()
        })
        .on_click(move |checked, _, cx| {
            let value = if *checked { &value_on } else { &value_off };
            pick(&scope, &input_key, value.clone(), cx);
        })
        .into_any_element()
}

fn choices_view(choice: &ChoiceInput, key: &str, current: &str, scope: &CardScope) -> AnyElement {
    let (scope, input_key) = (scope.clone(), key.to_owned());
    if choice.multi {
        return v_flex()
            .gap(px(CHOICE_GAP))
            .children(choice.choices.iter().enumerate().map(|(index, candidate)| {
                let (choice, scope, input_key, current, value) = (
                    choice.clone(),
                    scope.clone(),
                    input_key.clone(),
                    current.to_owned(),
                    candidate.value.clone(),
                );
                Checkbox::new(ElementId::Name(format!("{key}-choice-{index}").into()))
                    .label(candidate.title.clone())
                    .checked(choice.is_selected(&current, &value))
                    .on_click(move |_, _, cx| {
                        pick(&scope, &input_key, choice.toggled(&current, &value), cx)
                    })
            }))
            .into_any_element();
    }
    let selected = choice
        .choices
        .iter()
        .position(|candidate| candidate.value == current);
    let values: Vec<String> = choice
        .choices
        .iter()
        .map(|candidate| candidate.value.clone())
        .collect();
    RadioGroup::new(ElementId::Name(format!("{key}-choices").into()))
        .selected_index(selected)
        .children(choice.choices.iter().enumerate().map(|(index, candidate)| {
            Radio::new(ElementId::Name(format!("{key}-choice-{index}").into()))
                .label(candidate.title.clone())
        }))
        .on_click(move |index, _, cx| {
            if let Some(value) = values.get(*index) {
                pick(&scope, &input_key, value.clone(), cx);
            }
        })
        .into_any_element()
}

fn pick(scope: &CardScope, key: &str, value: String, cx: &mut App) {
    cx.global::<AppHandle>()
        .0
        .clone()
        .update(cx, |state, cx| state.pick_card_input(scope, key, value, cx));
}
