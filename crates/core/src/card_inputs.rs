use chrono::{NaiveDate, NaiveTime};
use regex::Regex;
use serde_json::{Map, Value};

const DATE_FORMAT: &str = "%Y-%m-%d";
const TIME_FORMAT: &str = "%H:%M";
const DEFAULT_VALUE_ON: &str = "true";
const DEFAULT_VALUE_OFF: &str = "false";
const CHOICE_SEPARATOR: char = ',';
const REQUIRED_MESSAGE: &str = "Required";
const INVALID_MESSAGE: &str = "Invalid value";
pub const DATE_PLACEHOLDER: &str = "YYYY-MM-DD";
pub const TIME_PLACEHOLDER: &str = "HH:MM";

#[derive(Debug, Clone, PartialEq)]
pub struct CardInput {
    pub id: String,
    pub label: Option<String>,
    pub required: bool,
    pub error_message: Option<String>,
    pub kind: CardInputKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CardInputKind {
    Text(TextInput),
    Number(NumberInput),
    Date(MomentInput),
    Time(MomentInput),
    Toggle(ToggleInput),
    Choice(ChoiceInput),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextStyle {
    Text,
    Tel,
    Url,
    Email,
    Password,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextInput {
    pub placeholder: Option<String>,
    pub value: String,
    pub multiline: bool,
    pub max_length: Option<usize>,
    pub style: TextStyle,
    pub regex: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NumberInput {
    pub placeholder: Option<String>,
    pub value: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MomentInput {
    pub value: String,
    pub min: Option<String>,
    pub max: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToggleInput {
    pub title: String,
    pub value: String,
    pub value_on: String,
    pub value_off: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChoiceStyle {
    Compact,
    Expanded,
    Filtered,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChoiceInput {
    pub choices: Vec<InputChoice>,
    pub multi: bool,
    pub style: ChoiceStyle,
    pub placeholder: Option<String>,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputChoice {
    pub title: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputError {
    pub id: String,
    pub message: String,
}

impl TextStyle {
    fn parse(value: Option<&str>) -> Self {
        match value.map(str::to_ascii_lowercase).as_deref() {
            Some("tel") => TextStyle::Tel,
            Some("url") => TextStyle::Url,
            Some("email") => TextStyle::Email,
            Some("password") => TextStyle::Password,
            _ => TextStyle::Text,
        }
    }
}

impl ChoiceStyle {
    fn parse(value: Option<&str>) -> Self {
        match value.map(str::to_ascii_lowercase).as_deref() {
            Some("expanded") => ChoiceStyle::Expanded,
            Some("filtered") => ChoiceStyle::Filtered,
            _ => ChoiceStyle::Compact,
        }
    }
}

impl ToggleInput {
    pub fn is_on(&self, current: &str) -> bool {
        current == self.value_on
    }

    pub fn flipped(&self, current: &str) -> String {
        if self.is_on(current) {
            self.value_off.clone()
        } else {
            self.value_on.clone()
        }
    }
}

impl ChoiceInput {
    pub fn selected<'current>(&self, current: &'current str) -> Vec<&'current str> {
        if self.multi {
            current
                .split(CHOICE_SEPARATOR)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .collect()
        } else {
            Some(current)
                .filter(|value| !value.is_empty())
                .into_iter()
                .collect()
        }
    }

    pub fn is_selected(&self, current: &str, choice_value: &str) -> bool {
        self.selected(current).contains(&choice_value)
    }

    pub fn toggled(&self, current: &str, choice_value: &str) -> String {
        if !self.multi {
            return choice_value.to_owned();
        }
        let selected = self.selected(current);
        let keep = |choice: &&InputChoice| {
            (choice.value == choice_value) != selected.contains(&choice.value.as_str())
        };
        self.choices
            .iter()
            .filter(keep)
            .map(|choice| choice.value.as_str())
            .collect::<Vec<_>>()
            .join(&CHOICE_SEPARATOR.to_string())
    }
}

impl CardInput {
    pub fn initial_value(&self) -> String {
        match &self.kind {
            CardInputKind::Text(text) => text.value.clone(),
            CardInputKind::Number(number) => number.value.clone(),
            CardInputKind::Date(moment) | CardInputKind::Time(moment) => moment.value.clone(),
            CardInputKind::Toggle(toggle) => toggle.value.clone(),
            CardInputKind::Choice(choice) => choice.value.clone(),
        }
    }

    pub fn validate(&self, value: &str) -> Option<String> {
        let message = || {
            self.error_message
                .clone()
                .filter(|message| !message.trim().is_empty())
        };
        if value.is_empty() {
            return (self.required)
                .then(|| message().unwrap_or_else(|| REQUIRED_MESSAGE.to_owned()));
        }
        let valid = match &self.kind {
            CardInputKind::Text(text) => text.accepts(value),
            CardInputKind::Number(number) => number.accepts(value),
            CardInputKind::Date(moment) => moment.accepts(value, parse_date),
            CardInputKind::Time(moment) => moment.accepts(value, parse_time),
            CardInputKind::Toggle(toggle) => !self.required || toggle.is_on(value),
            CardInputKind::Choice(_) => true,
        };
        (!valid).then(|| message().unwrap_or_else(|| INVALID_MESSAGE.to_owned()))
    }
}

impl TextInput {
    fn accepts(&self, value: &str) -> bool {
        self.max_length
            .is_none_or(|max_length| value.chars().count() <= max_length)
            && self
                .regex
                .as_deref()
                .is_none_or(|pattern| Regex::new(pattern).is_ok_and(|regex| regex.is_match(value)))
    }
}

impl NumberInput {
    fn accepts(&self, value: &str) -> bool {
        let Ok(number) = value.trim().parse::<f64>() else {
            return false;
        };
        number.is_finite()
            && self.min.is_none_or(|min| number >= min)
            && self.max.is_none_or(|max| number <= max)
    }
}

impl MomentInput {
    fn accepts<Moment: PartialOrd>(&self, value: &str, parse: fn(&str) -> Option<Moment>) -> bool {
        let Some(moment) = parse(value) else {
            return false;
        };
        self.min
            .as_deref()
            .and_then(parse)
            .is_none_or(|min| moment >= min)
            && self
                .max
                .as_deref()
                .and_then(parse)
                .is_none_or(|max| moment <= max)
    }
}

fn parse_date(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value, DATE_FORMAT).ok()
}

fn parse_time(value: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(value, TIME_FORMAT).ok()
}

pub fn collect_input_values(
    inputs: &[CardInput],
    value_of: impl Fn(&CardInput) -> String,
) -> Result<Map<String, Value>, Vec<InputError>> {
    let mut values = Map::new();
    let mut errors = Vec::new();
    for input in inputs {
        let value = value_of(input);
        match input.validate(&value) {
            Some(message) => errors.push(InputError {
                id: input.id.clone(),
                message,
            }),
            None => {
                values.insert(input.id.clone(), Value::String(value));
            }
        }
    }
    if errors.is_empty() {
        Ok(values)
    } else {
        Err(errors)
    }
}

pub fn merge_input_data(data: &Value, values: &Map<String, Value>) -> Value {
    match data {
        Value::Object(object) => {
            let mut merged = object.clone();
            merged.extend(values.clone());
            Value::Object(merged)
        }
        Value::Null => Value::Object(values.clone()),
        other => other.clone(),
    }
}

pub(crate) fn parse_input(value: &Value, input_type: &str) -> Option<CardInput> {
    let id = text_field(value, "id")?;
    let placeholder = text_field(value, "placeholder");
    let kind = match input_type {
        "Input.Text" => CardInputKind::Text(TextInput {
            placeholder,
            value: text_field(value, "value").unwrap_or_default(),
            multiline: bool_field(value, "isMultiline"),
            max_length: value
                .get("maxLength")
                .and_then(Value::as_u64)
                .filter(|max_length| *max_length > 0)
                .map(|max_length| max_length as usize),
            style: TextStyle::parse(value.get("style").and_then(Value::as_str)),
            regex: text_field(value, "regex"),
        }),
        "Input.Number" => CardInputKind::Number(NumberInput {
            placeholder,
            value: number_text(value.get("value")),
            min: value.get("min").and_then(Value::as_f64),
            max: value.get("max").and_then(Value::as_f64),
        }),
        "Input.Date" => CardInputKind::Date(parse_moment(value)),
        "Input.Time" => CardInputKind::Time(parse_moment(value)),
        "Input.Toggle" => {
            let value_on =
                text_field(value, "valueOn").unwrap_or_else(|| DEFAULT_VALUE_ON.to_owned());
            let value_off =
                text_field(value, "valueOff").unwrap_or_else(|| DEFAULT_VALUE_OFF.to_owned());
            CardInputKind::Toggle(ToggleInput {
                title: text_field(value, "title").unwrap_or_default(),
                value: text_field(value, "value").unwrap_or_else(|| value_off.clone()),
                value_on,
                value_off,
            })
        }
        "Input.ChoiceSet" => {
            let choices: Vec<InputChoice> = value
                .get("choices")?
                .as_array()?
                .iter()
                .filter_map(|choice| {
                    Some(InputChoice {
                        title: text_field(choice, "title")?,
                        value: text_field(choice, "value")?,
                    })
                })
                .collect();
            if choices.is_empty() {
                return None;
            }
            CardInputKind::Choice(ChoiceInput {
                choices,
                multi: bool_field(value, "isMultiSelect"),
                style: ChoiceStyle::parse(value.get("style").and_then(Value::as_str)),
                placeholder,
                value: text_field(value, "value").unwrap_or_default(),
            })
        }
        _ => return None,
    };
    Some(CardInput {
        id,
        label: text_field(value, "label"),
        required: bool_field(value, "isRequired"),
        error_message: text_field(value, "errorMessage"),
        kind,
    })
}

fn parse_moment(value: &Value) -> MomentInput {
    MomentInput {
        value: text_field(value, "value").unwrap_or_default(),
        min: text_field(value, "min"),
        max: text_field(value, "max"),
    }
}

fn text_field(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(str::to_owned)
}

fn bool_field(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn number_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::String(text)) => text.clone(),
        _ => String::new(),
    }
}
