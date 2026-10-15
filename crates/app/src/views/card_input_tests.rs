use std::sync::Arc;

use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, Render, TestAppContext, VisualTestContext,
    Window,
};
use serde_json::json;
use store::Store;
use teams_core::AdaptiveCard;

use crate::card_inputs::InputField;
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

fn typed_card() -> AdaptiveCard {
    let card = json!({
        "body": [
            {"type": "Input.Text", "id": "code", "maxLength": 3, "regex": "^[a-z]+$", "errorMessage": "Letters only"},
            {"type": "Input.Date", "id": "day", "value": "2026-11-02", "min": "2026-10-01", "max": "2026-12-31"},
            {"type": "Input.Time", "id": "hour", "value": "09:30", "min": "10:00", "errorMessage": "Too early"},
            {"type": "Input.Number", "id": "count", "value": 4},
            {"type": "Input.Rating", "id": "stars", "value": 2, "allowHalf": true}
        ],
        "actions": [{"type": "Action.Submit", "title": "Save", "data": {"action": "save"}}]
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
    open_card(cx, surface, form_card())
}

fn open_card(
    cx: &mut TestAppContext,
    surface: Surface,
    card: AdaptiveCard,
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

#[gpui_kit::test]
fn typing_past_the_max_length_is_ignored(cx: &mut TestAppContext) {
    let (cx, app, scope, _) = open_card(cx, Surface::Message, typed_card());
    let key = scope.input_key("code");
    let field = cx.update(|_, cx| app.read(cx).cards.input_field(&key).cloned());
    let Some(InputField::Line(line)) = field else {
        panic!("code is a single line field");
    };
    cx.update(|window, cx| line.update(cx, |state, cx| state.focus(window, cx)));
    cx.simulate_input("abcdef");
    assert_eq!(
        cx.update(|_, cx| app.read(cx).cards.input_text(&key, cx)),
        "abc"
    );
}

#[gpui_kit::test]
fn blur_validation_shows_the_error_message(cx: &mut TestAppContext) {
    let (cx, app, scope, card) = open_card(cx, Surface::Message, typed_card());
    let key = scope.input_key("code");
    let field = cx.update(|_, cx| app.read(cx).cards.input_field(&key).cloned());
    let Some(InputField::Line(line)) = field else {
        panic!("code is a single line field");
    };
    cx.update(|window, cx| line.update(cx, |state, cx| state.focus(window, cx)));
    cx.simulate_input("AB");
    let input = card.own_inputs().remove(0);
    cx.update(|_, cx| {
        app.update(cx, |state, cx| {
            state.validate_card_input(&scope, &key, &input, cx)
        })
    });
    assert_eq!(error_of(cx, &app, &key).as_deref(), Some("Letters only"));
}

#[gpui_kit::test]
fn date_time_and_rating_values_are_collected_with_their_types(cx: &mut TestAppContext) {
    let (cx, app, scope, card) = open_card(cx, Surface::Message, typed_card());
    let hour_key = scope.input_key("hour");
    assert!(task_for(cx, &app, &scope, &card, 0).is_none());
    assert_eq!(error_of(cx, &app, &hour_key).as_deref(), Some("Too early"));
    cx.update(|_, cx| {
        app.update(cx, |state, cx| {
            state.pick_card_input(&scope, &hour_key, "10:15".into(), cx);
            state.pick_card_input(&scope, &scope.input_key("stars"), "3.5".into(), cx);
        })
    });
    let sent = sent_value(task_for(cx, &app, &scope, &card, 0));
    assert_eq!(
        sent,
        json!({
            "action": "save", "code": "", "day": "2026-11-02", "hour": "10:15",
            "count": "4", "stars": 3.5
        })
    );
}
