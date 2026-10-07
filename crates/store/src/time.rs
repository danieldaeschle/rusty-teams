use chrono::{DateTime, Utc};

pub fn to_millis(time: DateTime<Utc>) -> i64 {
    time.timestamp_millis()
}

pub fn from_millis(millis: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(millis).unwrap_or_default()
}

pub fn optional_to_millis(time: Option<DateTime<Utc>>) -> Option<i64> {
    time.map(to_millis)
}

pub fn optional_from_millis(millis: Option<i64>) -> Option<DateTime<Utc>> {
    millis.map(from_millis)
}
