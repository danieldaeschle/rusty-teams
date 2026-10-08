use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::radio::{Radio, RadioGroup};
use gpui_kit::component::select::Select;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::{CardInput, CardInputKind, ChoiceInput, ToggleInput};

use crate::app_state::AppHandle;
use crate::card_inputs::{InputField, uses_select};
use crate::card_state::{CardScope, CardState};
use crate::theme;

const INPUT_GAP: f32 = 4.;
const LABEL_SIZE: f32 = 12.5;
const ERROR_SIZE: f32 = 12.;
const FIELD_RADIUS: f32 = 6.;
const CHOICE_GAP: f32 = 6.;

pub fn input_view(input: &CardInput, scope: &CardScope, state: &CardState) -> AnyElement {
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
                .child(control_view(input, &key, scope, state)),
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

fn control_view(input: &CardInput, key: &str, scope: &CardScope, state: &CardState) -> AnyElement {
    let current = state.input_value(key).to_owned();
    match &input.kind {
        CardInputKind::Toggle(toggle) => toggle_view(toggle, key, &current, scope),
        CardInputKind::Choice(choice) if !uses_select(choice) => {
            choices_view(choice, key, &current, scope)
        }
        _ => match state.input_field(key) {
            Some(InputField::Line(field)) => Input::new(field).into_any_element(),
            Some(InputField::Area(field)) => Textarea::new(field).into_any_element(),
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

fn toggle_view(toggle: &ToggleInput, key: &str, current: &str, scope: &CardScope) -> AnyElement {
    let (value_on, value_off) = (toggle.value_on.clone(), toggle.value_off.clone());
    let (scope, input_key) = (scope.clone(), key.to_owned());
    Checkbox::new(ElementId::Name(format!("{key}-toggle").into()))
        .label(toggle.title.clone())
        .checked(toggle.is_on(current))
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
