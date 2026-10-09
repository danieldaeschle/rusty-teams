use std::time::Duration;

use gpui_kit::component::IndexPath;
use gpui_kit::component::calendar::Matcher;
use gpui_kit::component::date_picker::{DatePickerEvent, DatePickerState};
use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::component::searchable_list::{
    SearchableListDelegate, SearchableListItem, SearchableVec,
};
use gpui_kit::component::select::{SelectEvent, SelectState};
use gpui_kit::component::time_field::{TimeFieldEvent, TimeFieldState};
use gpui_kit::*;
use regex::Regex;
use teams_core::{
    AdaptiveCard, CardAction, CardActionKind, CardInput, CardInputKind, ChoiceInput, ChoiceQuery,
    ChoiceStyle, InputChoice, TextStyle, collect_input_values, format_date, format_time,
};
use tokio::sync::oneshot;

use crate::app_state::{AppHandle, AppState};
use crate::card_actions::CardTask;
use crate::card_state::{CardScope, Surface, short_reason};
use crate::runtime;

const NUMBER_PATTERN: &str = r"^-?\d*\.?\d*$";
const AREA_MIN_ROWS: usize = 3;
const AREA_MAX_ROWS: usize = 8;
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(300);
const NOT_CONNECTED: &str = "Not connected";
const READ_ONLY: &str = "Read-only mode";
const SEARCH_FAILED: &str = "Search failed";
const INLINE_ACTION_SUFFIX: &str = "inline";

pub type SearchAnswer = Result<Vec<InputChoice>, String>;

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

pub struct RemoteSearch {
    scope: CardScope,
    key: String,
    query: ChoiceQuery,
}

pub struct ChoiceList {
    items: SearchableVec<ChoiceItem>,
    remote: Option<RemoteSearch>,
    search_task: Task<()>,
}

impl ChoiceList {
    fn new(items: Vec<ChoiceItem>, remote: Option<RemoteSearch>) -> Self {
        ChoiceList {
            items: SearchableVec::new(items),
            remote,
            search_task: Task::ready(()),
        }
    }

    fn found(choices: Vec<InputChoice>, remote: Option<RemoteSearch>) -> Self {
        ChoiceList::new(choice_item_list(&choices), remote)
    }
}

impl SearchableListDelegate for ChoiceList {
    type Item = ChoiceItem;

    fn items_count(&self, section: usize) -> usize {
        self.items.items_count(section)
    }

    fn item(&self, ix: IndexPath) -> Option<&ChoiceItem> {
        self.items.item(ix)
    }

    fn position<V>(&self, value: &V) -> Option<IndexPath>
    where
        ChoiceItem: SearchableListItem<Value = V>,
        V: PartialEq,
    {
        self.items.position(value)
    }

    fn perform_search(&mut self, query: &str, window: &mut Window, cx: &mut App) -> Task<()> {
        let Some(remote) = &self.remote else {
            return self.items.perform_search(query, window, cx);
        };
        if query.is_empty() {
            return Task::ready(());
        }
        let (scope, key, choice_query, query_text) = (
            remote.scope.clone(),
            remote.key.clone(),
            remote.query.clone(),
            query.to_owned(),
        );
        self.search_task = window.spawn(cx, async move |cx| {
            cx.background_executor().timer(SEARCH_DEBOUNCE).await;
            let app = cx.update(|_, cx| cx.global::<AppHandle>().0.clone()).ok();
            let Some(app) = app else {
                return;
            };
            let receiver = cx
                .update(|_, cx| {
                    app.update(cx, |state, _| {
                        state.card_choice_search(&scope, &choice_query, &query_text)
                    })
                })
                .ok();
            let answer = match receiver {
                Some(receiver) => receiver.await.unwrap_or_else(|_| Err(SEARCH_FAILED.into())),
                None => return,
            };
            cx.update(|window, cx| {
                app.update(cx, |state, cx| {
                    state.show_choice_results(&scope, &key, &choice_query, answer, window, cx)
                })
            })
            .ok();
        });
        Task::ready(())
    }
}

pub type ChoiceSelect = SelectState<ChoiceList>;

#[derive(Clone)]
pub enum InputField {
    Line(Entity<InputState>),
    Area(Entity<TextareaState>),
    Select(Entity<ChoiceSelect>),
    Date(Entity<DatePickerState>),
    Time(Entity<TimeFieldState>),
}

impl InputField {
    pub fn text(&self, cx: &App) -> Option<String> {
        match self {
            InputField::Line(state) => Some(state.read(cx).value().to_string()),
            InputField::Area(state) => Some(state.read(cx).value().to_string()),
            InputField::Select(state) => {
                Some(state.read(cx).selected_value().cloned().unwrap_or_default())
            }
            InputField::Date(_) | InputField::Time(_) => None,
        }
    }
}

pub struct FieldHandle {
    pub field: InputField,
    _subscriptions: Vec<Subscription>,
}

fn choice_item_list(choices: &[InputChoice]) -> Vec<ChoiceItem> {
    choices
        .iter()
        .map(|choice| ChoiceItem {
            title: choice.title.clone().into(),
            value: choice.value.clone(),
        })
        .collect()
}

pub fn choice_list(choice: &ChoiceInput, scope: &CardScope, key: &str) -> ChoiceList {
    let remote = choice.query.clone().map(|query| RemoteSearch {
        scope: scope.clone(),
        key: key.to_owned(),
        query,
    });
    let mut choices = choice.choices.clone();
    if remote.is_some() && !choice.value.is_empty() {
        let known = choices
            .iter()
            .any(|candidate| candidate.value == choice.value);
        if !known {
            choices.push(InputChoice {
                title: choice.value.clone(),
                value: choice.value.clone(),
            });
        }
    }
    ChoiceList::found(choices, remote)
}

pub fn uses_select(choice: &ChoiceInput) -> bool {
    choice.query.is_some() || (!choice.multi && choice.style != ChoiceStyle::Expanded)
}

pub fn length_pattern(max_length: usize) -> Option<Regex> {
    Regex::new(&format!("^(?s:.{{0,{max_length}}})$")).ok()
}

pub fn inline_action_key(input_key: &str) -> String {
    format!("{input_key}-{INLINE_ACTION_SUFFIX}")
}

impl AppState {
    pub fn ensure_card_inputs(
        &mut self,
        scope: &CardScope,
        card: &AdaptiveCard,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let inputs = card.all_inputs();
        for input in &inputs {
            let key = scope.input_key(&input.id);
            if self.cards.has_input(&key) {
                continue;
            }
            let handle = Self::create_field(scope, &key, input, &inputs, window, cx);
            self.cards.seed_input(key, input.initial_value(), handle);
        }
    }

    fn create_field(
        scope: &CardScope,
        key: &str,
        input: &CardInput,
        inputs: &[CardInput],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<FieldHandle> {
        let value = input.initial_value();
        let (scope, key) = (scope.clone(), key.to_owned());
        let validated = input.clone();
        match &input.kind {
            CardInputKind::Text(text) if text.multiline => {
                let placeholder = text.placeholder.clone().unwrap_or_default();
                let max_length = text.max_length;
                let state = cx.new(|cx| {
                    TextareaState::new(window, cx)
                        .auto_grow(AREA_MIN_ROWS, AREA_MAX_ROWS)
                        .placeholder(placeholder)
                        .default_value(value)
                });
                let subscription = cx.subscribe_in(&state, window, {
                    let (scope, key) = (scope.clone(), key.clone());
                    move |this, state, event: &InputEvent, window, cx| match event {
                        InputEvent::Change => {
                            let mut text = state.read(cx).value().to_string();
                            if let Some(max_length) = max_length
                                && text.chars().count() > max_length
                            {
                                text = text.chars().take(max_length).collect();
                                state.update(cx, |area, cx| {
                                    area.set_value(text.clone(), window, cx)
                                });
                            }
                            this.edit_card_input(&scope, &key, text, cx);
                        }
                        InputEvent::Blur => this.validate_card_input(&scope, &key, &validated, cx),
                        _ => {}
                    }
                });
                Some(FieldHandle {
                    field: InputField::Area(state),
                    _subscriptions: vec![subscription],
                })
            }
            CardInputKind::Text(_) | CardInputKind::Number(_) => {
                let (placeholder, masked, pattern, inline_action) = match &input.kind {
                    CardInputKind::Text(text) => (
                        text.placeholder.clone(),
                        text.style == TextStyle::Password,
                        text.max_length.and_then(length_pattern),
                        text.inline_action.clone(),
                    ),
                    CardInputKind::Number(number) => (
                        number.placeholder.clone(),
                        false,
                        Regex::new(NUMBER_PATTERN).ok(),
                        None,
                    ),
                    _ => (None, false, None, None),
                };
                let state = cx.new(|cx| {
                    let line = InputState::new(window, cx)
                        .placeholder(placeholder.unwrap_or_default())
                        .masked(masked)
                        .default_value(value);
                    match pattern {
                        Some(pattern) => line.pattern(pattern),
                        None => line,
                    }
                });
                let inputs = inputs.to_vec();
                let subscription = cx.subscribe(&state, {
                    let (scope, key) = (scope.clone(), key.clone());
                    move |this, state, event: &InputEvent, cx| match event {
                        InputEvent::Change => {
                            let text = state.read(cx).value().to_string();
                            this.edit_card_input(&scope, &key, text, cx);
                        }
                        InputEvent::Blur => this.validate_card_input(&scope, &key, &validated, cx),
                        InputEvent::PressEnter { .. } => {
                            if let Some(inline) = &inline_action {
                                this.run_inline_action(
                                    &scope,
                                    inline_action_key(&key),
                                    &inline.action,
                                    &inputs,
                                    cx,
                                );
                            }
                        }
                        InputEvent::Focus => {}
                    }
                });
                Some(FieldHandle {
                    field: InputField::Line(state),
                    _subscriptions: vec![subscription],
                })
            }
            CardInputKind::Date(moment) => {
                let (min, max) = (moment.min_date(), moment.max_date());
                let initial = moment.initial_date();
                let state = cx.new(|cx| {
                    let mut picker = DatePickerState::new(window, cx).date_format("%Y-%m-%d");
                    if min.is_some() || max.is_some() {
                        picker = picker.disabled_matcher(Matcher::interval(min, max));
                    }
                    if let Some(date) = initial {
                        picker.set_date(date, window, cx);
                    }
                    picker
                });
                let subscription = cx.subscribe(&state, {
                    move |this, _, event: &DatePickerEvent, cx| {
                        let DatePickerEvent::Change(picked) = event;
                        let text = picked.date().start().map(format_date).unwrap_or_default();
                        this.pick_card_input(&scope, &key, text, cx);
                        this.validate_card_input(&scope, &key, &validated, cx);
                    }
                });
                Some(FieldHandle {
                    field: InputField::Date(state),
                    _subscriptions: vec![subscription],
                })
            }
            CardInputKind::Time(moment) => {
                let initial = moment.initial_time();
                let state = cx.new(|cx| {
                    let mut field = TimeFieldState::new(window, cx);
                    if let Some(time) = initial {
                        field.set_time(time, window, cx);
                    }
                    field
                });
                let focus_handle = state.focus_handle(cx);
                let changed = cx.subscribe(&state, {
                    let (scope, key) = (scope.clone(), key.clone());
                    move |this, _, event: &TimeFieldEvent, cx| {
                        let TimeFieldEvent::Change(time) = event;
                        this.pick_card_input(&scope, &key, format_time(*time), cx);
                    }
                });
                let blurred = cx.on_blur(&focus_handle, window, move |this, _, cx| {
                    this.validate_card_input(&scope, &key, &validated, cx)
                });
                Some(FieldHandle {
                    field: InputField::Time(state),
                    _subscriptions: vec![changed, blurred],
                })
            }
            CardInputKind::Choice(choice) if uses_select(choice) => {
                let searchable = choice.style == ChoiceStyle::Filtered || choice.query.is_some();
                let list = choice_list(choice, &scope, &key);
                let selected = (0..list.items_count(0))
                    .find(|row| {
                        list.item(IndexPath::default().row(*row))
                            .is_some_and(|item| item.value == value)
                    })
                    .map(|row| IndexPath::default().row(row));
                let state = cx
                    .new(|cx| SelectState::new(list, selected, window, cx).searchable(searchable));
                let subscription = cx.subscribe(
                    &state,
                    move |this, _, event: &SelectEvent<ChoiceList>, cx| {
                        let SelectEvent::Confirm(picked) = event;
                        this.pick_card_input(&scope, &key, picked.clone().unwrap_or_default(), cx);
                    },
                );
                Some(FieldHandle {
                    field: InputField::Select(state),
                    _subscriptions: vec![subscription],
                })
            }
            CardInputKind::Choice(_) | CardInputKind::Toggle(_) | CardInputKind::Rating(_) => None,
        }
    }

    pub fn validate_card_input(
        &mut self,
        scope: &CardScope,
        key: &str,
        input: &CardInput,
        cx: &mut Context<Self>,
    ) {
        let value = self.cards.input_text(key, cx);
        match input.validate(&value) {
            Some(message) => self.cards.set_input_error(key, message),
            None => self.cards.clear_input_errors(&[key.to_owned()]),
        }
        self.announce_cards(scope, cx);
    }

    pub fn run_inline_action(
        &mut self,
        scope: &CardScope,
        action_key: String,
        action: &CardAction,
        inputs: &[CardInput],
        cx: &mut Context<Self>,
    ) {
        match &action.kind {
            CardActionKind::OpenUrl(url) => cx.open_url(url),
            _ => self.run_card_action(
                scope.clone(),
                action_key,
                action.clone(),
                inputs.to_vec(),
                cx,
            ),
        }
    }

    pub fn card_choice_search(
        &self,
        scope: &CardScope,
        query: &ChoiceQuery,
        query_text: &str,
    ) -> oneshot::Receiver<SearchAnswer> {
        if self.mode.demo {
            let (dataset, query_text) = (query.dataset.clone(), query_text.to_owned());
            return runtime::spawn(async move {
                Ok(crate::demo_input_cards::search(&dataset, &query_text))
            });
        }
        if self.mode.read_only {
            return runtime::spawn(async { Err(READ_ONLY.to_owned()) });
        }
        let Some(engine) = self.engine.clone() else {
            return runtime::spawn(async { Err(NOT_CONNECTED.to_owned()) });
        };
        let (conversation_id, message_id) =
            (scope.conversation_id.clone(), scope.message_id.clone());
        let (query, query_text) = (query.clone(), query_text.to_owned());
        runtime::spawn(async move {
            engine
                .card_search(&conversation_id, &message_id, &query, &query_text)
                .await
                .map_err(|error| short_reason(&error.to_string()))
        })
    }

    pub fn show_choice_results(
        &mut self,
        scope: &CardScope,
        key: &str,
        query: &ChoiceQuery,
        answer: SearchAnswer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match answer {
            Ok(choices) => {
                if self.cards.input_error(key) == Some(SEARCH_FAILED) {
                    self.cards.clear_input_errors(&[key.to_owned()]);
                    self.announce_cards(scope, cx);
                }
                if let Some(InputField::Select(select)) = self.cards.input_field(key).cloned() {
                    let remote = RemoteSearch {
                        scope: scope.clone(),
                        key: key.to_owned(),
                        query: query.clone(),
                    };
                    select.update(cx, |select, cx| {
                        select.set_items(ChoiceList::found(choices, Some(remote)), window, cx);
                        cx.notify();
                    });
                }
            }
            Err(_) => {
                self.cards.set_input_error(key, SEARCH_FAILED.to_owned());
                self.announce_cards(scope, cx);
            }
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
}
