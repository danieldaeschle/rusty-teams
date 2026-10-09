use std::fmt::Display;
use std::sync::LazyLock;

use chrono::{DateTime, Local, TimeZone};
use regex::{Captures, Regex};

const COMPACT_FORMAT: &str = "%Y-%m-%d";
const SHORT_FORMAT: &str = "%a, %b %-d, %Y";
const LONG_FORMAT: &str = "%A, %B %-d, %Y";
const TIME_FORMAT: &str = "%H:%M";

static DATE_TIME_FUNCTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:\{\{\s*)?\b(DATE|TIME)\(\s*([^,()\s]+)\s*(?:,\s*(SHORT|LONG|COMPACT)\s*)?\)(?:\s*\}\})?")
        .unwrap()
});

pub fn format_card_dates(text: &str) -> String {
    format_card_dates_in(text, &Local)
}

pub fn format_card_dates_in<Zone>(text: &str, zone: &Zone) -> String
where
    Zone: TimeZone,
    Zone::Offset: Display,
{
    DATE_TIME_FUNCTION
        .replace_all(text, |captures: &Captures| {
            format_function(captures, zone).unwrap_or_else(|| captures[0].to_owned())
        })
        .into_owned()
}

fn format_function<Zone>(captures: &Captures, zone: &Zone) -> Option<String>
where
    Zone: TimeZone,
    Zone::Offset: Display,
{
    let moment = DateTime::parse_from_rfc3339(&captures[2])
        .ok()?
        .with_timezone(zone);
    let pattern = match (&captures[1], captures.get(3).map(|style| style.as_str())) {
        ("TIME", _) => TIME_FORMAT,
        (_, Some("SHORT")) => SHORT_FORMAT,
        (_, Some("LONG")) => LONG_FORMAT,
        _ => COMPACT_FORMAT,
    };
    Some(moment.format(pattern).to_string())
}
