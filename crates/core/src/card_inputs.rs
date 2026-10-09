use chrono::{NaiveDate, NaiveTime};
use regex::Regex;
use serde_json::{Map, Number, Value, json};

use crate::adaptive_card::{CardAction, CardActionKind, parse_action};

const DATE_FORMAT: &str = "%Y-%m-%d";
const TIME_FORMAT: &str = "%H:%M";
const DEFAULT_VALUE_ON: &str = "true";
const DEFAULT_VALUE_OFF: &str = "false";
const CHOICE_SEPARATOR: char = ',';
const REQUIRED_MESSAGE: &str = "Required";
const INVALID_MESSAGE: &str = "Invalid value";
const INLINE_ACTION_TITLE: &str = "Go";
const DEFAULT_SEARCH_COUNT: usize = 15;
const DEFAULT_RATING_MAX: u32 = 5;
const MAX_RATING_MAX: u32 = 10;
const WHOLE_NUMBER_LIMIT: f64 = 1e15;
const DATA_QUERY_TYPE: &str = "Data.Query";
pub const SEARCH_INVOKE_NAME: &str = "application/search";
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
    Rating(RatingInput),
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
    pub inline_action: Option<Box<InlineAction>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InlineAction {
    pub action: CardAction,
    pub icon_url: Option<String>,
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
    pub wrap: bool,
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
    pub query: Option<ChoiceQuery>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceQuery {
    pub dataset: String,
    pub count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RatingColor {
    Neutral,
    Marigold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RatingSize {
    Medium,
    Large,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RatingStyle {
    Default,
    Compact,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RatingInput {
    pub max: u32,
    pub value: String,
    pub color: RatingColor,
    pub size: RatingSize,
    pub allow_half: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RatingDisplay {
    pub value: f64,
    pub max: u32,
    pub count: Option<u64>,
    pub size: RatingSize,
    pub style: RatingStyle,
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

impl RatingColor {
    fn parse(value: Option<&str>) -> Self {
        match value.map(str::to_ascii_lowercase).as_deref() {
            Some("marigold") => RatingColor::Marigold,
            _ => RatingColor::Neutral,
        }
    }
}

impl RatingSize {
    fn parse(value: Option<&str>) -> Self {
        match value.map(str::to_ascii_lowercase).as_deref() {
            Some("large") => RatingSize::Large,
            _ => RatingSize::Medium,
        }
    }
}

impl RatingStyle {
    fn parse(value: Option<&str>) -> Self {
        match value.map(str::to_ascii_lowercase).as_deref() {
            Some("compact") => RatingStyle::Compact,
            _ => RatingStyle::Default,
        }
    }
}

impl RatingInput {
    pub fn rating(&self, current: &str) -> f64 {
        current.trim().parse::<f64>().unwrap_or(0.)
    }

    pub fn value_text(&self, rating: f64) -> String {
        if rating <= 0. {
            String::new()
        } else {
            number_value(rating.min(f64::from(self.max))).to_string()
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
            CardInputKind::Rating(rating) => rating.value.clone(),
        }
    }

    pub fn submitted_value(&self, value: &str) -> Value {
        match (&self.kind, value.trim().parse::<f64>()) {
            (CardInputKind::Rating(_), Ok(number)) if number.is_finite() => {
                Value::Number(number_value(number))
            }
            _ => Value::String(value.to_owned()),
        }
    }

    fn is_blank(&self, value: &str) -> bool {
        match &self.kind {
            CardInputKind::Rating(rating) => value.is_empty() || rating.rating(value) <= 0.,
            _ => value.is_empty(),
        }
    }

    pub fn validate(&self, value: &str) -> Option<String> {
        let message = || {
            self.error_message
                .clone()
                .filter(|message| !message.trim().is_empty())
        };
        if self.is_blank(value) {
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
            CardInputKind::Rating(rating) => !self.required || rating.rating(value) > 0.,
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
    pub fn initial_date(&self) -> Option<NaiveDate> {
        parse_date(&self.value)
    }

    pub fn min_date(&self) -> Option<NaiveDate> {
        self.min.as_deref().and_then(parse_date)
    }

    pub fn max_date(&self) -> Option<NaiveDate> {
        self.max.as_deref().and_then(parse_date)
    }

    pub fn initial_time(&self) -> Option<NaiveTime> {
        parse_time(&self.value)
    }

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

pub fn format_date(date: NaiveDate) -> String {
    date.format(DATE_FORMAT).to_string()
}

pub fn format_time(time: NaiveTime) -> String {
    time.format(TIME_FORMAT).to_string()
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
                values.insert(input.id.clone(), input.submitted_value(&value));
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

pub fn search_request_value(query: &ChoiceQuery, query_text: &str) -> Value {
    json!({
        "queryText": query_text,
        "queryOptions": {"skip": 0, "top": query.count},
        "dataset": query.dataset,
    })
}

pub fn parse_search_results(response_value: &Value) -> Vec<InputChoice> {
    response_value
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|result| {
            Some(InputChoice {
                title: text_field(result, "title")?,
                value: text_field(result, "value")?,
            })
        })
        .collect()
}

pub(crate) fn parse_rating(value: &Value) -> RatingDisplay {
    RatingDisplay {
        value: value.get("value").and_then(Value::as_f64).unwrap_or(0.),
        max: rating_max(value),
        count: value.get("count").and_then(Value::as_u64),
        size: RatingSize::parse(value.get("size").and_then(Value::as_str)),
        style: RatingStyle::parse(value.get("style").and_then(Value::as_str)),
    }
}

fn rating_max(value: &Value) -> u32 {
    value
        .get("max")
        .and_then(Value::as_u64)
        .map_or(DEFAULT_RATING_MAX, |max| {
            max.clamp(1, MAX_RATING_MAX.into()) as u32
        })
}

fn parse_choice_query(value: &Value) -> Option<ChoiceQuery> {
    let data = value.get("choices.data")?;
    if text_field(data, "type")? != DATA_QUERY_TYPE {
        return None;
    }
    Some(ChoiceQuery {
        dataset: text_field(data, "dataset")?,
        count: data
            .get("count")
            .and_then(Value::as_u64)
            .filter(|count| *count > 0)
            .map_or(DEFAULT_SEARCH_COUNT, |count| count as usize),
    })
}

fn parse_inline_action(value: &Value) -> Option<Box<InlineAction>> {
    let mut action_value = value.get("inlineAction")?.clone();
    if text_field(&action_value, "title").is_none_or(|title| title.trim().is_empty()) {
        action_value["title"] = json!(INLINE_ACTION_TITLE);
    }
    let action = parse_action(&action_value)?;
    matches!(
        action.kind,
        CardActionKind::Submit(_) | CardActionKind::Execute(_) | CardActionKind::OpenUrl(_)
    )
    .then(|| {
        Box::new(InlineAction {
            action,
            icon_url: text_field(&action_value, "iconUrl"),
        })
    })
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
            inline_action: parse_inline_action(value),
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
                wrap: bool_field(value, "wrap"),
            })
        }
        "Input.ChoiceSet" => {
            let query = parse_choice_query(value);
            let choices: Vec<InputChoice> = value
                .get("choices")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|choice| {
                    Some(InputChoice {
                        title: text_field(choice, "title")?,
                        value: text_field(choice, "value")?,
                    })
                })
                .collect();
            if choices.is_empty() && query.is_none() {
                return None;
            }
            CardInputKind::Choice(ChoiceInput {
                choices,
                multi: bool_field(value, "isMultiSelect"),
                style: ChoiceStyle::parse(value.get("style").and_then(Value::as_str)),
                placeholder,
                value: text_field(value, "value").unwrap_or_default(),
                query,
            })
        }
        "Input.Rating" => {
            let max = rating_max(value);
            let rating = RatingInput {
                max,
                value: String::new(),
                color: RatingColor::parse(value.get("color").and_then(Value::as_str)),
                size: RatingSize::parse(value.get("size").and_then(Value::as_str)),
                allow_half: bool_field(value, "allowHalf"),
            };
            let initial = value.get("value").and_then(Value::as_f64).unwrap_or(0.);
            CardInputKind::Rating(RatingInput {
                value: rating.value_text(initial),
                ..rating
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

fn number_value(number: f64) -> Number {
    if number.fract() == 0. && number.abs() < WHOLE_NUMBER_LIMIT {
        Number::from(number as i64)
    } else {
        Number::from_f64(number).unwrap_or_else(|| Number::from(0))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn input(value: Value) -> CardInput {
        let input_type = value["type"].as_str().unwrap().to_owned();
        parse_input(&value, &input_type).unwrap()
    }

    #[test]
    fn required_empty_text_shows_the_error_message_or_a_default() {
        let custom = input(
            json!({"type": "Input.Text", "id": "a", "isRequired": true, "errorMessage": "Fill me"}),
        );
        assert_eq!(custom.validate("").as_deref(), Some("Fill me"));
        let plain = input(json!({"type": "Input.Text", "id": "a", "isRequired": true}));
        assert_eq!(plain.validate("").as_deref(), Some(REQUIRED_MESSAGE));
        assert_eq!(plain.validate("x"), None);
    }

    #[test]
    fn text_regex_and_max_length_are_checked() {
        let ticket = input(
            json!({"type": "Input.Text", "id": "t", "regex": "^[A-Z]{3}-\\d+$", "maxLength": 6, "errorMessage": "Bad ticket"}),
        );
        assert_eq!(ticket.validate("ABC-12"), None);
        assert_eq!(ticket.validate("abc-12").as_deref(), Some("Bad ticket"));
        assert_eq!(ticket.validate("ABC-1234").as_deref(), Some("Bad ticket"));
        assert_eq!(ticket.validate(""), None);
    }

    #[test]
    fn number_bounds_are_inclusive() {
        let servers = input(json!({"type": "Input.Number", "id": "n", "min": 1, "max": 20}));
        assert_eq!(servers.validate("1"), None);
        assert_eq!(servers.validate("20"), None);
        assert_eq!(servers.validate("0").as_deref(), Some(INVALID_MESSAGE));
        assert_eq!(servers.validate("21").as_deref(), Some(INVALID_MESSAGE));
        assert_eq!(servers.validate("1.").as_deref(), None);
        assert_eq!(servers.validate("-").as_deref(), Some(INVALID_MESSAGE));
    }

    #[test]
    fn date_and_time_bounds_are_checked() {
        let day = input(
            json!({"type": "Input.Date", "id": "d", "min": "2026-10-01", "max": "2026-12-31"}),
        );
        assert_eq!(day.validate("2026-10-01"), None);
        assert_eq!(day.validate("2026-12-31"), None);
        assert!(day.validate("2026-09-30").is_some());
        assert!(day.validate("2027-01-01").is_some());
        assert!(day.validate("tomorrow").is_some());
        let start = input(json!({"type": "Input.Time", "id": "s", "min": "06:00", "max": "20:00"}));
        assert_eq!(start.validate("06:00"), None);
        assert!(start.validate("05:59").is_some());
        assert!(start.validate("20:01").is_some());
    }

    #[test]
    fn moments_are_formatted_as_the_bot_expects() {
        let day = NaiveDate::from_ymd_opt(2026, 3, 7).unwrap();
        let time = NaiveTime::from_hms_opt(9, 5, 0).unwrap();
        assert_eq!(format_date(day), "2026-03-07");
        assert_eq!(format_time(time), "09:05");
        let moment = MomentInput {
            value: "2026-03-07".into(),
            min: Some("2026-03-01".into()),
            max: None,
        };
        assert_eq!(moment.initial_date(), Some(day));
        assert_eq!(moment.min_date(), NaiveDate::from_ymd_opt(2026, 3, 1));
        assert_eq!(moment.max_date(), None);
    }

    #[test]
    fn rating_parses_with_defaults_and_clamps_the_value() {
        let rating = input(
            json!({"type": "Input.Rating", "id": "r", "value": 9, "color": "marigold", "size": "large", "allowHalf": true}),
        );
        let CardInputKind::Rating(parsed) = &rating.kind else {
            panic!("rating expected");
        };
        assert_eq!(parsed.max, 5);
        assert_eq!(parsed.value, "5");
        assert_eq!(parsed.color, RatingColor::Marigold);
        assert_eq!(parsed.size, RatingSize::Large);
        assert!(parsed.allow_half);
        let ten = input(json!({"type": "Input.Rating", "id": "r", "max": 40}));
        let CardInputKind::Rating(parsed) = &ten.kind else {
            panic!("rating expected");
        };
        assert_eq!(parsed.max, MAX_RATING_MAX);
        assert_eq!(parsed.value, "");
    }

    #[test]
    fn rating_is_submitted_as_a_number_and_required_means_above_zero() {
        let rating = input(
            json!({"type": "Input.Rating", "id": "r", "isRequired": true, "allowHalf": true}),
        );
        assert_eq!(rating.submitted_value("3"), json!(3));
        assert_eq!(rating.submitted_value("3.5"), json!(3.5));
        assert!(rating.validate("").is_some());
        assert!(rating.validate("0").is_some());
        assert_eq!(rating.validate("0.5"), None);
        let CardInputKind::Rating(parsed) = &rating.kind else {
            panic!("rating expected");
        };
        assert_eq!(parsed.value_text(2.5), "2.5");
        assert_eq!(parsed.value_text(0.), "");
    }

    #[test]
    fn numbers_and_text_are_submitted_as_text() {
        let count = input(json!({"type": "Input.Number", "id": "n"}));
        assert_eq!(count.submitted_value("4"), json!("4"));
        assert_eq!(count.submitted_value("4.25"), json!("4.25"));
        assert_eq!(count.submitted_value(""), json!(""));
        let text = input(json!({"type": "Input.Text", "id": "t"}));
        assert_eq!(text.submitted_value("4"), json!("4"));
    }

    #[test]
    fn collected_values_use_their_submitted_types() {
        let inputs = vec![
            input(json!({"type": "Input.Rating", "id": "r", "value": 4})),
            input(json!({"type": "Input.Number", "id": "n", "value": 7})),
        ];
        let values = collect_input_values(&inputs, CardInput::initial_value).unwrap();
        assert_eq!(Value::Object(values), json!({"r": 4, "n": "7"}));
    }

    #[test]
    fn data_query_choice_sets_keep_their_input_without_static_choices() {
        let choice = input(json!({
            "type": "Input.ChoiceSet", "id": "c",
            "choices.data": {"type": "Data.Query", "dataset": "people", "count": 7}
        }));
        let CardInputKind::Choice(parsed) = &choice.kind else {
            panic!("choice set expected");
        };
        assert!(parsed.choices.is_empty());
        assert_eq!(
            parsed.query,
            Some(ChoiceQuery {
                dataset: "people".into(),
                count: 7
            })
        );
        assert!(
            parse_input(
                &json!({"type": "Input.ChoiceSet", "id": "c"}),
                "Input.ChoiceSet"
            )
            .is_none()
        );
    }

    #[test]
    fn search_request_carries_query_options_and_dataset() {
        let query = ChoiceQuery {
            dataset: "people".into(),
            count: 15,
        };
        assert_eq!(
            search_request_value(&query, "ad"),
            json!({"queryText": "ad", "queryOptions": {"skip": 0, "top": 15}, "dataset": "people"})
        );
    }

    #[test]
    fn search_results_keep_only_complete_title_value_pairs() {
        let response = json!({"results": [
            {"title": "Ada", "value": "ada"},
            {"title": "No value"},
            {"title": "Bob", "value": "bob"}
        ]});
        let titles: Vec<String> = parse_search_results(&response)
            .into_iter()
            .map(|choice| choice.value)
            .collect();
        assert_eq!(titles, ["ada", "bob"]);
        assert!(parse_search_results(&Value::Null).is_empty());
    }

    #[test]
    fn inline_action_defaults_its_title_and_rejects_other_kinds() {
        let with_action = input(json!({
            "type": "Input.Text", "id": "t",
            "inlineAction": {"type": "Action.Submit", "iconUrl": "https://example.com/i.png", "data": {"go": 1}}
        }));
        let CardInputKind::Text(text) = &with_action.kind else {
            panic!("text expected");
        };
        let inline = text.inline_action.as_ref().unwrap();
        assert_eq!(inline.action.title, INLINE_ACTION_TITLE);
        assert_eq!(
            inline.icon_url.as_deref(),
            Some("https://example.com/i.png")
        );
        let toggling = input(json!({
            "type": "Input.Text", "id": "t",
            "inlineAction": {"type": "Action.ToggleVisibility", "title": "x", "targetElements": ["a"]}
        }));
        let CardInputKind::Text(text) = &toggling.kind else {
            panic!("text expected");
        };
        assert!(text.inline_action.is_none());
    }

    #[test]
    fn toggle_wrap_is_parsed() {
        let toggle =
            input(json!({"type": "Input.Toggle", "id": "t", "title": "Long", "wrap": true}));
        let CardInputKind::Toggle(parsed) = &toggle.kind else {
            panic!("toggle expected");
        };
        assert!(parsed.wrap);
    }

    #[test]
    fn rating_element_reads_value_count_and_style() {
        let display = parse_rating(
            &json!({"type": "Rating", "value": 4.5, "count": 120, "style": "compact", "size": "large"}),
        );
        assert_eq!(display.value, 4.5);
        assert_eq!(display.max, 5);
        assert_eq!(display.count, Some(120));
        assert_eq!(display.style, RatingStyle::Compact);
        assert_eq!(display.size, RatingSize::Large);
    }
}
