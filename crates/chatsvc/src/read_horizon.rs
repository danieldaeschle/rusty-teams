use chrono::{DateTime, Utc};
use serde_json::Value;

pub const PROPERTY_NAME: &str = "consumptionhorizon";

/// Read state of one conversation as chatsvc lists it, judged like the Teams worker (`hasUnreadMessages`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReadHorizon {
    pub consumed_until_ms: i64,
    pub bookmark_ms: i64,
    pub bookmark_message_set: bool,
    pub last_message_ms: i64,
    pub last_message_from: Option<String>,
}

impl ReadHorizon {
    pub fn from_conversation(conversation: &Value) -> Self {
        let properties = &conversation["properties"];
        let horizon = parse_horizon(properties["consumptionhorizon"].as_str());
        let bookmark = parse_horizon(properties["consumptionHorizonBookmark"].as_str());
        let last_message = &conversation["lastMessage"];
        ReadHorizon {
            consumed_until_ms: horizon.arrival_ms,
            bookmark_ms: bookmark.arrival_ms,
            bookmark_message_set: bookmark.message_set,
            last_message_ms: last_message_ms(conversation),
            last_message_from: last_message["from"]
                .as_str()
                .and_then(|from| from.rsplit('/').next())
                .filter(|from| !from.is_empty())
                .map(str::to_owned),
        }
    }

    pub fn is_unread(&self, my_user_id: &str) -> bool {
        if self.bookmark_ms != 0 && self.bookmark_message_set {
            return true;
        }
        let unread = self.bookmark_ms != 0 || self.last_message_ms > self.consumed_until_ms;
        let from_me = self
            .last_message_from
            .as_deref()
            .is_some_and(|from| !my_user_id.is_empty() && from.contains(my_user_id));
        unread && !from_me
    }

    pub fn last_message_at(&self) -> Option<DateTime<Utc>> {
        (self.last_message_ms > 0)
            .then(|| DateTime::from_timestamp_millis(self.last_message_ms))
            .flatten()
    }
}

/// The value Teams writes when a conversation is read: last arrival time, now, client message id.
pub fn read_up_to(last_message_at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    format!(
        "{};{};0",
        last_message_at.timestamp_millis(),
        now.timestamp_millis()
    )
}

struct ParsedHorizon {
    arrival_ms: i64,
    message_set: bool,
}

fn parse_horizon(text: Option<&str>) -> ParsedHorizon {
    let parts: Vec<&str> = text.unwrap_or_default().split(';').collect();
    let number = |part: &str| part.trim().parse::<i64>().ok();
    let unset = ParsedHorizon {
        arrival_ms: 0,
        message_set: false,
    };
    let [arrival, timestamp, message_id] = parts.as_slice() else {
        return unset;
    };
    let message_id = message_id.trim();
    match (number(arrival), number(timestamp)) {
        (Some(arrival_ms), Some(_)) if !message_id.is_empty() => ParsedHorizon {
            arrival_ms,
            message_set: message_id != "0",
        },
        _ => unset,
    }
}

fn last_message_ms(conversation: &Value) -> i64 {
    if let Some(time) = conversation["lastMessageTimeUtc"].as_i64() {
        return time;
    }
    conversation["lastMessage"]["originalarrivaltime"]
        .as_str()
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .map_or(0, |time| time.timestamp_millis())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const ME: &str = "8:orgid:me";

    fn conversation(
        horizon: Option<&str>,
        bookmark: Option<&str>,
        last: Option<(&str, &str)>,
    ) -> Value {
        json!({
            "id": "19:channel@thread.skype",
            "properties": {
                "consumptionhorizon": horizon,
                "consumptionHorizonBookmark": bookmark,
            },
            "lastMessage": last.map(|(arrival, from)| json!({
                "originalarrivaltime": arrival,
                "from": format!("https://teams.cloud.microsoft/api/chatsvc/emea/v1/users/ME/contacts/{from}"),
            })).unwrap_or_else(|| json!({})),
        })
    }

    fn unread(value: &Value) -> bool {
        ReadHorizon::from_conversation(value).is_unread(ME)
    }

    #[test]
    fn message_after_the_horizon_is_unread() {
        let value = conversation(
            Some("1791884440747;1791885752824;1131155174560809354"),
            None,
            Some(("2026-10-16T09:42:25.6780000Z", "8:orgid:other")),
        );
        assert!(unread(&value));
    }

    #[test]
    fn message_at_or_before_the_horizon_is_read() {
        let value = conversation(
            Some("1792171970947;1792171971513;1792171970879"),
            Some(""),
            Some(("2026-10-16T17:32:50.9470000Z", "8:orgid:other")),
        );
        assert!(!unread(&value));
    }

    #[test]
    fn missing_horizon_counts_every_message_as_unread() {
        let value = conversation(None, None, Some(("2026-10-16T20:30:58.901Z", "28:bot")));
        assert!(unread(&value));
    }

    #[test]
    fn channel_without_messages_is_read() {
        assert!(!unread(&conversation(None, None, None)));
    }

    #[test]
    fn own_last_message_is_read() {
        let value = conversation(Some("1;2;3"), None, Some(("2026-10-16T09:42:25.678Z", ME)));
        assert!(!unread(&value));
    }

    #[test]
    fn bookmark_with_a_message_marks_unread_even_for_own_message() {
        let value = conversation(
            Some("1792171970947;1792171971513;1"),
            Some("1792171970000;1792171971513;42"),
            Some(("2026-10-16T17:32:50.9470000Z", ME)),
        );
        assert!(unread(&value));
    }

    #[test]
    fn empty_bookmark_from_teams_is_ignored() {
        let value = conversation(
            Some("1792171970947;1792171971513;1"),
            Some("0;1697436994849;0"),
            Some(("2026-10-16T17:32:50.9470000Z", "8:orgid:other")),
        );
        assert!(!unread(&value));
    }

    #[test]
    fn malformed_horizon_counts_as_none() {
        let value = conversation(
            Some("garbage"),
            None,
            Some(("2026-10-16T09:42:25.678Z", "8:orgid:other")),
        );
        assert!(unread(&value));
    }

    #[test]
    fn read_value_carries_the_last_arrival_and_now() {
        let last = DateTime::from_timestamp_millis(1792182658901).unwrap();
        let now = DateTime::from_timestamp_millis(1792190000000).unwrap();
        assert_eq!(read_up_to(last, now), "1792182658901;1792190000000;0");
    }
}
