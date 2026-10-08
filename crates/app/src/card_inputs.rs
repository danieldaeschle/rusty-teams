use gpui_kit::component::IndexPath;
use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::component::searchable_list::{SearchableListItem, SearchableVec};
use gpui_kit::component::select::{SelectEvent, SelectState};
use gpui_kit::*;
use regex::Regex;
use teams_core::{
    AdaptiveCard, CardAction, CardActionKind, CardInput, CardInputKind, ChoiceInput, ChoiceStyle,
    DATE_PLACEHOLDER, TIME_PLACEHOLDER, TextStyle, collect_input_values,
};

use crate::app_state::AppState;
use crate::card_actions::CardTask;
use crate::card_state::{CardScope, Surface};

const NUMBER_PATTERN: &str = r"^-?\d*\.?\d*$";
const AREA_MIN_ROWS: usize = 3;
const AREA_MAX_ROWS: usize = 8;

#[derive(Clone)]
pub struct ChoiceItem {
    pub title: SharedString,
    pub value: String,
}

impl SearchableListItem for ChoiceItem {
    type Value = String;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &String {
        &self.value
    }
}

pub type ChoiceSelect = SelectState<SearchableVec<ChoiceItem>>;

#[derive(Clone)]
pub enum InputField {
    Line(Entity<InputState>),
    Area(Entity<TextareaState>),
    Select(Entity<ChoiceSelect>),
}

impl InputField {
    pub fn text(&self, cx: &App) -> String {
        match self {
            InputField::Line(state) => state.read(cx).value().to_string(),
            InputField::Area(state) => state.read(cx).value().to_string(),
            InputField::Select(state) => {
                state.read(cx).selected_value().cloned().unwrap_or_default()
            }
        }
    }
}

pub struct FieldHandle {
    pub field: InputField,
    _subscription: Subscription,
}

pub fn choice_items(choice: &ChoiceInput) -> SearchableVec<ChoiceItem> {
    SearchableVec::new(
        choice
            .choices
            .iter()
            .map(|choice| ChoiceItem {
                title: choice.title.clone().into(),
                value: choice.value.clone(),
            })
            .collect::<Vec<_>>(),
    )
}

pub fn uses_select(choice: &ChoiceInput) -> bool {
    !choice.multi && choice.style != ChoiceStyle::Expanded
}

impl AppState {
    pub fn ensure_card_inputs(
        &mut self,
        scope: &CardScope,
        card: &AdaptiveCard,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for input in card.all_inputs() {
            let key = scope.input_key(&input.id);
            if self.cards.has_input(&key) {
                continue;
            }
            let handle = Self::create_field(scope, &key, &input, window, cx);
            self.cards.seed_input(key, input.initial_value(), handle);
        }
    }

    fn create_field(
        scope: &CardScope,
        key: &str,
        input: &CardInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<FieldHandle> {
        let value = input.initial_value();
        let (scope, key) = (scope.clone(), key.to_owned());
        match &input.kind {
            CardInputKind::Text(text) if text.multiline => {
                let placeholder = text.placeholder.clone().unwrap_or_default();
                let state = cx.new(|cx| {
                    TextareaState::new(window, cx)
                        .auto_grow(AREA_MIN_ROWS, AREA_MAX_ROWS)
                        .placeholder(placeholder)
                        .default_value(value)
                });
                let subscription =
                    cx.subscribe(&state, move |this, state, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::Change) {
                            let text = state.read(cx).value().to_string();
                            this.edit_card_input(&scope, &key, text, cx);
                        }
                    });
                Some(FieldHandle {
                    field: InputField::Area(state),
                    _subscription: subscription,
                })
            }
            CardInputKind::Text(_)
            | CardInputKind::Number(_)
            | CardInputKind::Date(_)
            | CardInputKind::Time(_) => {
                let (placeholder, masked, numeric) = match &input.kind {
                    CardInputKind::Text(text) => (
                        text.placeholder.clone(),
                        text.style == TextStyle::Password,
                        false,
                    ),
                    CardInputKind::Number(number) => (number.placeholder.clone(), false, true),
                    CardInputKind::Date(_) => (Some(DATE_PLACEHOLDER.to_owned()), false, false),
                    _ => (Some(TIME_PLACEHOLDER.to_owned()), false, false),
                };
                let state = cx.new(|cx| {
                    let line = InputState::new(window, cx)
                        .placeholder(placeholder.unwrap_or_default())
                        .masked(masked)
                        .default_value(value);
                    match Regex::new(NUMBER_PATTERN) {
                        Ok(pattern) if numeric => line.pattern(pattern),
                        _ => line,
                    }
                });
                let subscription =
                    cx.subscribe(&state, move |this, state, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::Change) {
                            let text = state.read(cx).value().to_string();
                            this.edit_card_input(&scope, &key, text, cx);
                        }
                    });
                Some(FieldHandle {
                    field: InputField::Line(state),
                    _subscription: subscription,
                })
            }
            CardInputKind::Choice(choice) if uses_select(choice) => {
                let searchable = choice.style == ChoiceStyle::Filtered;
                let selected = choice
                    .choices
                    .iter()
                    .position(|candidate| candidate.value == value)
                    .map(|row| IndexPath::default().row(row));
                let state = cx.new(|cx| {
                    SelectState::new(choice_items(choice), selected, window, cx)
                        .searchable(searchable)
                });
                let subscription = cx.subscribe(
                    &state,
                    move |this, _, event: &SelectEvent<SearchableVec<ChoiceItem>>, cx| {
                        let SelectEvent::Confirm(picked) = event;
                        this.pick_card_input(&scope, &key, picked.clone().unwrap_or_default(), cx);
                    },
                );
                Some(FieldHandle {
                    field: InputField::Select(state),
                    _subscription: subscription,
                })
            }
            CardInputKind::Choice(_) | CardInputKind::Toggle(_) => None,
        }
    }

    pub fn edit_card_input(
        &mut self,
        scope: &CardScope,
        key: &str,
        value: String,
        cx: &mut Context<Self>,
    ) {
        if self.cards.set_input_value(key, value) {
            self.announce_cards(scope, cx);
        }
    }

    pub fn pick_card_input(
        &mut self,
        scope: &CardScope,
        key: &str,
        value: String,
        cx: &mut Context<Self>,
    ) {
        self.cards.set_input_value(key, value);
        self.announce_cards(scope, cx);
    }

    pub fn card_task(
        &mut self,
        scope: &CardScope,
        action: CardAction,
        inputs: &[CardInput],
        cx: &mut Context<Self>,
    ) -> Option<CardTask> {
        let submits = matches!(
            action.kind,
            CardActionKind::Submit(_) | CardActionKind::Execute(_)
        );
        let values = if submits && action.collects_inputs {
            let keys: Vec<String> = inputs
                .iter()
                .map(|input| scope.input_key(&input.id))
                .collect();
            self.cards.clear_input_errors(&keys);
            let collected = collect_input_values(inputs, |input| {
                self.cards.input_text(&scope.input_key(&input.id), cx)
            });
            match collected {
                Ok(values) => values,
                Err(errors) => {
                    for error in errors {
                        self.cards
                            .set_input_error(&scope.input_key(&error.id), error.message);
                    }
                    self.announce_cards(scope, cx);
                    return None;
                }
            }
        } else {
            serde_json::Map::new()
        };
        let action = action.with_inputs(&values);
        Some(match (&scope.surface, &action.kind) {
            (Surface::Dialog, CardActionKind::Submit(submit)) => {
                CardTask::Submit(submit.data.clone())
            }
            _ => CardTask::Action(action),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui_kit::{
        AppContext as _, Context, Entity, IntoElement, Render, TestAppContext, VisualTestContext,
        Window,
    };
    use serde_json::json;
    use store::Store;
    use teams_core::AdaptiveCard;

    use super::InputField;
    use crate::app_state::{AppHandle, AppState, Mode};
    use crate::card_actions::CardTask;
    use crate::card_state::{CardScope, Surface};
    use crate::views::adaptive_card::card_view;

    struct Harness {
        app: Entity<AppState>,
        scope: CardScope,
        card: AdaptiveCard,
    }

    impl Render for Harness {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.app.update(cx, |state, cx| {
                state.ensure_card_inputs(&self.scope, &self.card, window, cx)
            });
            card_view(&self.card, &self.scope, cx)
        }
    }

    fn form_card() -> AdaptiveCard {
        let card = json!({
            "body": [
                {"type": "Input.Text", "id": "name", "label": "Name", "isRequired": true, "errorMessage": "Name needed"},
                {"type": "Input.Toggle", "id": "agree", "title": "Agree"}
            ],
            "actions": [
                {"type": "Action.Submit", "title": "Save", "data": {"action": "save"}},
                {"type": "Action.Submit", "title": "Skip", "associatedInputs": "none", "data": {"action": "skip"}}
            ]
        });
        AdaptiveCard::parse(&card.to_string()).unwrap()
    }

    fn open(
        cx: &mut TestAppContext,
        surface: Surface,
    ) -> (
        &mut VisualTestContext,
        Entity<AppState>,
        CardScope,
        AdaptiveCard,
    ) {
        cx.update(gpui_kit::init);
        let store = Arc::new(Store::open_in_memory().unwrap());
        let app = cx.update(|cx| cx.new(|_| AppState::new(store, Mode::default())));
        cx.update(|cx| cx.set_global(AppHandle(app.clone())));
        let scope = CardScope {
            surface,
            ..CardScope::message("19:chat@thread.v2", "m1", 0)
        };
        let card = form_card();
        let (_, window_cx) = cx.add_window_view({
            let (app, scope, card) = (app.clone(), scope.clone(), card.clone());
            move |_, _| Harness { app, scope, card }
        });
        window_cx.update(|window, cx| window.draw(cx).clear(cx));
        (window_cx, app, scope, card)
    }

    fn focus_name_field(cx: &mut VisualTestContext, app: &Entity<AppState>, scope: &CardScope) {
        let key = scope.input_key("name");
        let field = cx.update(|_, cx| app.read(cx).cards.input_field(&key).cloned());
        let Some(InputField::Line(line)) = field else {
            panic!("name is a single line field");
        };
        cx.update(|window, cx| line.update(cx, |state, cx| state.focus(window, cx)));
    }

    fn task_for(
        cx: &mut VisualTestContext,
        app: &Entity<AppState>,
        scope: &CardScope,
        card: &AdaptiveCard,
        action_index: usize,
    ) -> Option<CardTask> {
        let action = card.actions[action_index].clone();
        let inputs = card.own_inputs();
        cx.update(|_, cx| app.update(cx, |state, cx| state.card_task(scope, action, &inputs, cx)))
    }

    fn object(pairs: &[(&str, &str)]) -> serde_json::Value {
        serde_json::Value::Object(
            pairs
                .iter()
                .map(|(key, value)| ((*key).to_owned(), serde_json::Value::from(*value)))
                .collect(),
        )
    }

    fn error_of(cx: &mut VisualTestContext, app: &Entity<AppState>, key: &str) -> Option<String> {
        cx.update(|_, cx| app.read(cx).cards.input_error(key).map(str::to_owned))
    }

    fn type_name(
        cx: &mut VisualTestContext,
        app: &Entity<AppState>,
        scope: &CardScope,
        text: &str,
    ) {
        focus_name_field(cx, app, scope);
        cx.simulate_input(text);
    }

    fn switch_agree(cx: &mut VisualTestContext, app: &Entity<AppState>, scope: &CardScope) {
        let key = scope.input_key("agree");
        cx.update(|_, cx| {
            app.update(cx, |state, cx| {
                state.pick_card_input(scope, &key, "true".into(), cx)
            })
        });
    }

    fn sent_value(task: Option<CardTask>) -> serde_json::Value {
        match task {
            Some(CardTask::Action(action)) => action.invoke_payload().unwrap().value,
            Some(CardTask::Submit(data)) => data,
            None => serde_json::Value::Null,
        }
    }

    #[gpui_kit::test]
    fn typed_text_and_toggle_are_sent_with_the_action_data(cx: &mut TestAppContext) {
        let (cx, app, scope, card) = open(cx, Surface::Message);
        type_name(cx, &app, &scope, "Ada");
        switch_agree(cx, &app, &scope);
        let sent = sent_value(task_for(cx, &app, &scope, &card, 0));
        assert_eq!(
            sent,
            object(&[("action", "save"), ("name", "Ada"), ("agree", "true")])
        );
    }

    #[gpui_kit::test]
    fn an_empty_required_input_blocks_the_send_until_it_changes(cx: &mut TestAppContext) {
        let (cx, app, scope, card) = open(cx, Surface::Message);
        assert!(task_for(cx, &app, &scope, &card, 0).is_none());
        let key = scope.input_key("name");
        assert_eq!(error_of(cx, &app, &key).as_deref(), Some("Name needed"));
        type_name(cx, &app, &scope, "A");
        assert_eq!(error_of(cx, &app, &key), None);
        assert!(task_for(cx, &app, &scope, &card, 0).is_some());
    }

    #[gpui_kit::test]
    fn associated_inputs_none_skips_collection_and_validation(cx: &mut TestAppContext) {
        let (cx, app, scope, card) = open(cx, Surface::Message);
        let sent = sent_value(task_for(cx, &app, &scope, &card, 1));
        assert_eq!(sent, object(&[("action", "skip")]));
    }

    #[gpui_kit::test]
    fn a_dialog_submit_carries_the_collected_values(cx: &mut TestAppContext) {
        let (cx, app, scope, card) = open(cx, Surface::Dialog);
        type_name(cx, &app, &scope, "Ada");
        let sent = sent_value(task_for(cx, &app, &scope, &card, 0));
        assert_eq!(
            sent,
            object(&[("action", "save"), ("name", "Ada"), ("agree", "false")])
        );
    }
}
