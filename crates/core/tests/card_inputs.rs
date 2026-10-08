use serde_json::{Value, json};
use teams_core::{
    AdaptiveCard, CardAction, CardActionKind, CardElement, CardInput, CardInputKind, ChoiceStyle,
    TextStyle, collect_input_values, merge_input_data,
};

fn card(body: Value, actions: Value) -> AdaptiveCard {
    AdaptiveCard::parse(&json!({"body": body, "actions": actions}).to_string()).unwrap()
}

fn inputs(body: Value) -> Vec<CardInput> {
    card(body, json!([])).own_inputs()
}

fn one(body: Value) -> CardInput {
    let mut parsed = inputs(json!([body]));
    assert_eq!(parsed.len(), 1);
    parsed.remove(0)
}

fn submit(data: Value, associated: Option<&str>) -> CardAction {
    let mut action = json!({"type": "Action.Submit", "title": "Go", "data": data});
    if let Some(associated) = associated {
        action["associatedInputs"] = json!(associated);
    }
    card(json!([{"type": "TextBlock", "text": "x"}]), json!([action]))
        .actions
        .remove(0)
}

fn values(pairs: Value) -> serde_json::Map<String, Value> {
    pairs.as_object().unwrap().clone()
}

#[test]
fn text_input_keeps_every_field() {
    let input = one(json!({
        "type": "Input.Text", "id": "name", "label": "Name", "placeholder": "Your name",
        "value": "Ada", "isMultiline": true, "maxLength": 20, "style": "Password",
        "regex": "^[A-Z]", "isRequired": true, "errorMessage": "Start with a capital"
    }));
    assert_eq!(input.id, "name");
    assert_eq!(input.label.as_deref(), Some("Name"));
    assert!(input.required);
    assert_eq!(input.error_message.as_deref(), Some("Start with a capital"));
    let CardInputKind::Text(text) = &input.kind else {
        panic!("text input expected");
    };
    assert_eq!(text.placeholder.as_deref(), Some("Your name"));
    assert_eq!(text.value, "Ada");
    assert!(text.multiline);
    assert_eq!(text.max_length, Some(20));
    assert_eq!(text.style, TextStyle::Password);
    assert_eq!(text.regex.as_deref(), Some("^[A-Z]"));
}

#[test]
fn number_input_reads_numeric_value_and_range() {
    let input = one(json!({"type": "Input.Number", "id": "n", "value": 5, "min": 1, "max": 10.5}));
    let CardInputKind::Number(number) = &input.kind else {
        panic!("number input expected");
    };
    assert_eq!(number.value, "5");
    assert_eq!((number.min, number.max), (Some(1.), Some(10.5)));
}

#[test]
fn date_and_time_inputs_keep_their_bounds() {
    let date = one(
        json!({"type": "Input.Date", "id": "d", "value": "2026-01-02", "min": "2026-01-01", "max": "2026-12-31"}),
    );
    let CardInputKind::Date(moment) = &date.kind else {
        panic!("date input expected");
    };
    assert_eq!(moment.value, "2026-01-02");
    assert_eq!(moment.min.as_deref(), Some("2026-01-01"));
    let time = one(json!({"type": "Input.Time", "id": "t", "value": "09:30"}));
    assert!(matches!(time.kind, CardInputKind::Time(_)));
    assert_eq!(time.initial_value(), "09:30");
}

#[test]
fn toggle_defaults_to_true_false_and_off() {
    let input = one(json!({"type": "Input.Toggle", "id": "t", "title": "Agree"}));
    let CardInputKind::Toggle(toggle) = &input.kind else {
        panic!("toggle expected");
    };
    assert_eq!(toggle.title, "Agree");
    assert_eq!(
        (toggle.value_on.as_str(), toggle.value_off.as_str()),
        ("true", "false")
    );
    assert_eq!(input.initial_value(), "false");
    assert_eq!(toggle.flipped("false"), "true");
    assert_eq!(toggle.flipped("true"), "false");
}

#[test]
fn toggle_with_custom_values_starts_on_its_value() {
    let input = one(
        json!({"type": "Input.Toggle", "id": "t", "title": "x", "valueOn": "yes", "valueOff": "no", "value": "yes"}),
    );
    assert_eq!(input.initial_value(), "yes");
}

#[test]
fn choice_set_parses_style_and_multi_select() {
    let input = one(json!({
        "type": "Input.ChoiceSet", "id": "c", "style": "expanded", "isMultiSelect": true,
        "value": "a,b", "placeholder": "Pick",
        "choices": [{"title": "A", "value": "a"}, {"title": "B", "value": "b"}, {"title": "C", "value": "c"}]
    }));
    let CardInputKind::Choice(choice) = &input.kind else {
        panic!("choice set expected");
    };
    assert_eq!(choice.style, ChoiceStyle::Expanded);
    assert!(choice.multi);
    assert_eq!(choice.choices.len(), 3);
    assert_eq!(choice.selected("a,b"), vec!["a", "b"]);
    assert_eq!(choice.toggled("a,b", "c"), "a,b,c");
    assert_eq!(choice.toggled("a,b", "a"), "b");
    assert_eq!(choice.toggled("", "b"), "b");
}

#[test]
fn single_choice_replaces_the_selection() {
    let input = one(json!({
        "type": "Input.ChoiceSet", "id": "c", "style": "filtered",
        "choices": [{"title": "A", "value": "a"}, {"title": "B", "value": "b"}]
    }));
    let CardInputKind::Choice(choice) = &input.kind else {
        panic!("choice set expected");
    };
    assert_eq!(choice.style, ChoiceStyle::Filtered);
    assert_eq!(choice.toggled("a", "b"), "b");
    assert!(choice.selected("").is_empty());
}

#[test]
fn inputs_without_id_or_choices_and_unknown_types_fall_back() {
    let parsed = card(
        json!([
            {"type": "Input.Text"},
            {"type": "Input.ChoiceSet", "id": "c", "choices": []},
            {"type": "Input.Rating", "id": "r", "fallbackText": "Rating is not supported"}
        ]),
        json!([]),
    );
    assert!(parsed.own_inputs().is_empty());
    assert_eq!(parsed.items.len(), 1);
    assert!(matches!(parsed.items[0].element, CardElement::Text(_)));
}

#[test]
fn nested_inputs_are_found_in_containers_and_columns_but_not_show_cards() {
    let parsed = card(
        json!([
            {"type": "Input.Text", "id": "a"},
            {"type": "Container", "items": [{"type": "Input.Text", "id": "b"}]},
            {"type": "ColumnSet", "columns": [{"type": "Column", "items": [{"type": "Input.Text", "id": "c"}]}]},
            {"type": "ActionSet", "actions": [
                {"type": "Action.ShowCard", "title": "More", "card": {"body": [{"type": "Input.Text", "id": "d"}]}}
            ]}
        ]),
        json!([]),
    );
    let own: Vec<String> = parsed
        .own_inputs()
        .into_iter()
        .map(|input| input.id)
        .collect();
    assert_eq!(own, ["a", "b", "c"]);
    let all: Vec<String> = parsed
        .all_inputs()
        .into_iter()
        .map(|input| input.id)
        .collect();
    assert_eq!(all, ["a", "b", "c", "d"]);
}

#[test]
fn required_input_rejects_empty_with_default_or_custom_message() {
    let plain = one(json!({"type": "Input.Text", "id": "a", "isRequired": true}));
    assert_eq!(plain.validate("").as_deref(), Some("Required"));
    assert_eq!(plain.validate("x"), None);
    let custom = one(
        json!({"type": "Input.Text", "id": "a", "isRequired": true, "errorMessage": "Tell us"}),
    );
    assert_eq!(custom.validate("").as_deref(), Some("Tell us"));
    let optional = one(json!({"type": "Input.Text", "id": "a"}));
    assert_eq!(optional.validate(""), None);
}

#[test]
fn text_regex_and_max_length_are_validated() {
    let input = one(json!({"type": "Input.Text", "id": "a", "regex": "^\\d+$", "maxLength": 3}));
    assert_eq!(input.validate("12"), None);
    assert_eq!(input.validate("ab").as_deref(), Some("Invalid value"));
    assert_eq!(input.validate("1234").as_deref(), Some("Invalid value"));
}

#[test]
fn number_range_and_format_are_validated() {
    let input = one(json!({"type": "Input.Number", "id": "n", "min": 1, "max": 10}));
    assert_eq!(input.validate("5.5"), None);
    assert_eq!(input.validate("0").as_deref(), Some("Invalid value"));
    assert_eq!(input.validate("11").as_deref(), Some("Invalid value"));
    assert_eq!(input.validate("abc").as_deref(), Some("Invalid value"));
}

#[test]
fn date_and_time_format_and_bounds_are_validated() {
    let date =
        one(json!({"type": "Input.Date", "id": "d", "min": "2026-01-10", "max": "2026-01-20"}));
    assert_eq!(date.validate("2026-01-15"), None);
    assert!(date.validate("2026-01-09").is_some());
    assert!(date.validate("2026-01-21").is_some());
    assert!(date.validate("15.01.2026").is_some());
    assert!(date.validate("2026-02-30").is_some());
    let time = one(json!({"type": "Input.Time", "id": "t", "min": "09:00", "max": "17:00"}));
    assert_eq!(time.validate("09:30"), None);
    assert!(time.validate("08:59").is_some());
    assert!(time.validate("25:00").is_some());
}

#[test]
fn required_toggle_must_be_on() {
    let input = one(json!({"type": "Input.Toggle", "id": "t", "title": "x", "isRequired": true}));
    assert_eq!(input.validate("false").as_deref(), Some("Invalid value"));
    assert_eq!(input.validate("true"), None);
}

#[test]
fn collecting_returns_every_error_or_all_values() {
    let parsed = inputs(json!([
        {"type": "Input.Text", "id": "a", "isRequired": true},
        {"type": "Input.Text", "id": "b", "isRequired": true, "errorMessage": "Needed"},
        {"type": "Input.Text", "id": "c"}
    ]));
    let errors = collect_input_values(&parsed, |input| {
        if input.id == "c" {
            "x".into()
        } else {
            String::new()
        }
    })
    .unwrap_err();
    assert_eq!(errors.len(), 2);
    assert_eq!(errors[0].id, "a");
    assert_eq!(errors[1].message, "Needed");
    let collected = collect_input_values(&parsed, |input| format!("v-{}", input.id)).unwrap();
    assert_eq!(
        Value::Object(collected),
        json!({"a": "v-a", "b": "v-b", "c": "v-c"})
    );
}

#[test]
fn merge_keeps_string_data_and_builds_objects_otherwise() {
    let inputs = values(json!({"a": "1"}));
    assert_eq!(
        merge_input_data(&json!({"k": 1, "a": "old"}), &inputs),
        json!({"k": 1, "a": "1"})
    );
    assert_eq!(merge_input_data(&Value::Null, &inputs), json!({"a": "1"}));
    assert_eq!(merge_input_data(&json!("plain"), &inputs), json!("plain"));
}

#[test]
fn submit_payload_carries_the_merged_inputs() {
    let action = submit(json!({"action": "save"}), None);
    assert!(action.collects_inputs);
    let payload = action
        .with_inputs(&values(json!({"name": "Ada"})))
        .invoke_payload()
        .unwrap();
    assert_eq!(payload.name, "messageback");
    assert_eq!(payload.value, json!({"action": "save", "name": "Ada"}));
}

#[test]
fn submit_without_data_becomes_the_input_object() {
    let action = card(
        json!([{"type": "TextBlock", "text": "x"}]),
        json!([{"type": "Action.Submit", "title": "Go"}]),
    )
    .actions
    .remove(0);
    let payload = action
        .with_inputs(&values(json!({"name": "Ada"})))
        .invoke_payload()
        .unwrap();
    assert_eq!(payload.value, json!({"name": "Ada"}));
}

#[test]
fn submit_with_string_data_ignores_the_inputs() {
    let action = submit(json!("go"), None);
    let payload = action
        .with_inputs(&values(json!({"name": "Ada"})))
        .invoke_payload()
        .unwrap();
    assert_eq!(payload.value, json!("go"));
}

#[test]
fn associated_inputs_none_is_parsed() {
    assert!(!submit(json!({}), Some("none")).collects_inputs);
    assert!(submit(json!({}), Some("auto")).collects_inputs);
}

#[test]
fn message_back_sends_the_merged_object_as_value() {
    let action = submit(
        json!({"id": 7, "msteams": {"type": "messageBack", "text": "hi", "value": "old"}}),
        None,
    );
    let payload = action
        .with_inputs(&values(json!({"name": "Ada"})))
        .invoke_payload()
        .unwrap();
    assert_eq!(payload.value, json!({"id": 7, "name": "Ada"}));
    let without = action
        .with_inputs(&values(json!({})))
        .invoke_payload()
        .unwrap();
    assert_eq!(without.value, json!("old"));
}

#[test]
fn task_fetch_merges_inputs_and_drops_the_settings() {
    let action = submit(json!({"msteams": {"type": "task/fetch"}, "k": 1}), None);
    let payload = action
        .with_inputs(&values(json!({"name": "Ada"})))
        .invoke_payload()
        .unwrap();
    assert_eq!(payload.name, "task/fetch");
    assert_eq!(
        payload.value,
        json!({"data": {"k": 1, "name": "Ada", "type": "task/fetch"}, "context": {"theme": "dark"}})
    );
}

#[test]
fn execute_merges_inputs_into_the_action_data() {
    let action = card(
        json!([{"type": "TextBlock", "text": "x"}]),
        json!([{"type": "Action.Execute", "title": "Go", "verb": "save", "data": {"k": 1}}]),
    )
    .actions
    .remove(0);
    let sent = action.with_inputs(&values(json!({"name": "Ada"})));
    let CardActionKind::Execute(execute) = &sent.kind else {
        panic!("execute expected");
    };
    assert_eq!(execute.data, json!({"k": 1, "name": "Ada"}));
    let payload = sent.invoke_payload().unwrap();
    assert_eq!(
        payload.value["action"]["data"],
        json!({"k": 1, "name": "Ada"})
    );
}
