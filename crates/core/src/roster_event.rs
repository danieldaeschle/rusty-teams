use chrono::{DateTime, Datelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use store::{MemberRecord, MessageRecord};

const MEMBERS_ADDED_TYPE: &str = "#microsoft.graph.membersAddedEventMessageDetail";
const MEMBERS_DELETED_TYPE: &str = "#microsoft.graph.membersDeletedEventMessageDetail";
const MEMBERS_LEFT_TYPE: &str = "#microsoft.graph.membersLeftEventMessageDetail";
const UNRESOLVED_NAME_PREFIX: &str = "orgid:";
const DAY_SECONDS: i64 = 86_400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RosterChange {
    Added,
    Removed,
    Left,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SharedHistory {
    #[default]
    None,
    Days(u32),
    All,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RosterEvent {
    pub change: RosterChange,
    pub initiator_id: Option<String>,
    pub initiator_name: Option<String>,
    pub member_ids: Vec<String>,
    pub member_names: Vec<Option<String>>,
    #[serde(default)]
    pub history: SharedHistory,
}

#[derive(Serialize, Deserialize)]
struct StoredRoster {
    roster: RosterEvent,
}

impl RosterEvent {
    pub fn from_graph(detail: &Value, created_at: Option<DateTime<Utc>>) -> Option<RosterEvent> {
        let kind = detail.get("@odata.type")?.as_str()?;
        let members: Vec<(String, Option<String>)> = detail
            .get("members")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|member| {
                let user_id = member.get("id")?.as_str()?.to_owned();
                Some((user_id, display_name(member)))
            })
            .collect();
        let initiator = detail
            .pointer("/initiator/user")
            .or_else(|| detail.pointer("/initiator/application"));
        let initiator_id = initiator
            .and_then(|identity| identity.get("id"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let only_initiator =
            members.len() == 1 && initiator_id.as_deref() == Some(members[0].0.as_str());
        let change = match kind {
            MEMBERS_ADDED_TYPE => RosterChange::Added,
            MEMBERS_LEFT_TYPE => RosterChange::Left,
            MEMBERS_DELETED_TYPE if only_initiator => RosterChange::Left,
            MEMBERS_DELETED_TYPE => RosterChange::Removed,
            _ => return None,
        };
        let history = match change {
            RosterChange::Added => shared_history(
                detail
                    .get("visibleHistoryStartDateTime")
                    .and_then(Value::as_str),
                created_at,
            ),
            _ => SharedHistory::None,
        };
        let (member_ids, member_names) = members.into_iter().unzip();
        Some(RosterEvent {
            change,
            initiator_name: initiator.and_then(display_name),
            initiator_id,
            member_ids,
            member_names,
            history,
        })
    }

    pub fn from_json(json: &str) -> Option<RosterEvent> {
        serde_json::from_str::<StoredRoster>(json)
            .ok()
            .map(|stored| stored.roster)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(&StoredRoster {
            roster: self.clone(),
        })
        .unwrap_or_default()
    }

    /// Teams wording; `name_of` gets each user id with the name the event carries.
    pub fn text(&self, name_of: impl Fn(Option<&str>, Option<&str>) -> String) -> String {
        let initiator = name_of(self.initiator_id.as_deref(), self.initiator_name.as_deref());
        let targets: Vec<String> = self
            .member_ids
            .iter()
            .zip(&self.member_names)
            .map(|(user_id, name)| name_of(Some(user_id), name.as_deref()))
            .collect();
        let targets = join_targets(&targets);
        match self.change {
            RosterChange::Left => format!("{initiator} left the chat."),
            RosterChange::Removed => format!("{initiator} removed {targets} from the chat."),
            RosterChange::Added => match self.history {
                SharedHistory::None => format!("{initiator} added {targets} to the chat."),
                SharedHistory::All => {
                    format!("{initiator} added {targets} to the chat and shared all chat history.")
                }
                SharedHistory::Days(1) => format!(
                    "{initiator} added {targets} to the chat and shared chat history from the past day."
                ),
                SharedHistory::Days(days) => format!(
                    "{initiator} added {targets} to the chat and shared chat history from the past {days} days."
                ),
            },
        }
    }
}

pub fn roster_event(record: &MessageRecord) -> Option<RosterEvent> {
    RosterEvent::from_json(record.call_event_json.as_deref()?)
}

pub fn merge_members(members: &mut Vec<MemberRecord>, added: &[MemberRecord]) {
    for person in added {
        let known = members
            .iter()
            .any(|member| member.user_id.is_some() && member.user_id == person.user_id);
        if !known {
            members.push(person.clone());
        }
    }
}

fn join_targets(targets: &[String]) -> String {
    match targets {
        [] => String::new(),
        [only] => only.clone(),
        [first, second] => format!("{first} and {second}"),
        [first, rest @ ..] => format!("{first} and {} others", rest.len()),
    }
}

fn display_name(identity: &Value) -> Option<String> {
    identity
        .get("displayName")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty() && !name.starts_with(UNRESOLVED_NAME_PREFIX))
        .map(str::to_owned)
}

fn shared_history(visible_from: Option<&str>, created_at: Option<DateTime<Utc>>) -> SharedHistory {
    let Some(visible_from) = visible_from.and_then(|text| DateTime::parse_from_rfc3339(text).ok())
    else {
        return SharedHistory::None;
    };
    if visible_from.year() <= 1 {
        return SharedHistory::All;
    }
    let Some(created_at) = created_at else {
        return SharedHistory::None;
    };
    let seconds = (created_at - visible_from.with_timezone(&Utc)).num_seconds();
    match (seconds + DAY_SECONDS / 2) / DAY_SECONDS {
        days if days >= 1 => SharedHistory::Days(u32::try_from(days).unwrap_or(u32::MAX)),
        _ => SharedHistory::None,
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde_json::json;

    use super::*;

    fn created() -> Option<DateTime<Utc>> {
        Some(Utc.with_ymd_and_hms(2026, 10, 17, 9, 0, 0).unwrap())
    }

    fn added(members: Value, visible_from: Value) -> RosterEvent {
        RosterEvent::from_graph(
            &json!({
                "@odata.type": MEMBERS_ADDED_TYPE,
                "visibleHistoryStartDateTime": visible_from,
                "members": members,
                "initiator": {"user": {"id": "me", "displayName": "Daniel"}},
            }),
            created(),
        )
        .unwrap()
    }

    fn plain_names(user_id: Option<&str>, name: Option<&str>) -> String {
        name.or(user_id).unwrap_or("Unknown").to_owned()
    }

    #[test]
    fn added_without_history_reads_like_teams() {
        let event = added(
            json!([{"id": "ann", "displayName": "Ann"}]),
            json!("2026-10-17T09:00:00Z"),
        );
        assert_eq!(event.history, SharedHistory::None);
        assert_eq!(event.text(plain_names), "Daniel added Ann to the chat.");
    }

    #[test]
    fn shared_history_is_all_or_rounded_days() {
        let all = added(json!([{"id": "ann"}]), json!("0001-01-01T00:00:00Z"));
        assert_eq!(all.history, SharedHistory::All);
        assert_eq!(
            all.text(plain_names),
            "Daniel added ann to the chat and shared all chat history."
        );
        let one = added(json!([{"id": "ann"}]), json!("2026-10-16T08:59:58Z"));
        assert_eq!(
            one.text(plain_names),
            "Daniel added ann to the chat and shared chat history from the past day."
        );
        let week = added(json!([{"id": "ann"}]), json!("2026-10-10T09:00:00Z"));
        assert_eq!(week.history, SharedHistory::Days(7));
    }

    #[test]
    fn targets_join_with_and_then_count_the_rest() {
        let two = added(json!([{"id": "ann"}, {"id": "bob"}]), Value::Null);
        assert_eq!(
            two.text(plain_names),
            "Daniel added ann and bob to the chat."
        );
        let four = added(
            json!([{"id": "ann"}, {"id": "bob"}, {"id": "cid"}, {"id": "dan"}]),
            Value::Null,
        );
        assert_eq!(
            four.text(plain_names),
            "Daniel added ann and 3 others to the chat."
        );
    }

    #[test]
    fn self_removal_is_leaving() {
        let detail = json!({
            "@odata.type": MEMBERS_DELETED_TYPE,
            "members": [{"id": "me"}],
            "initiator": {"user": {"id": "me", "displayName": "Daniel"}},
        });
        let event = RosterEvent::from_graph(&detail, created()).unwrap();
        assert_eq!(event.change, RosterChange::Left);
        assert_eq!(event.text(plain_names), "Daniel left the chat.");
        let mut removed = detail.clone();
        removed["members"] = json!([{"id": "ann", "displayName": "Ann"}]);
        let event = RosterEvent::from_graph(&removed, created()).unwrap();
        assert_eq!(event.text(plain_names), "Daniel removed Ann from the chat.");
    }

    #[test]
    fn stored_json_round_trips_and_is_not_a_call_event() {
        let event = added(json!([{"id": "ann"}]), Value::Null);
        let json = event.to_json();
        assert_eq!(RosterEvent::from_json(&json), Some(event));
        assert!(crate::CallEvent::from_json(&json).is_none());
        assert!(RosterEvent::from_json("{\"phase\":\"Ended\"}").is_none());
    }

    #[test]
    fn other_events_are_not_roster_changes() {
        let joined = json!({"@odata.type": "#microsoft.graph.membersJoinedEventMessageDetail"});
        assert!(RosterEvent::from_graph(&joined, created()).is_none());
    }

    #[test]
    fn merging_skips_known_members() {
        let member = |user_id: &str| MemberRecord {
            user_id: Some(user_id.to_owned()),
            display_name: user_id.to_owned(),
        };
        let mut members = vec![member("me"), member("ann")];
        merge_members(&mut members, &[member("ann"), member("bob")]);
        let ids: Vec<_> = members
            .iter()
            .filter_map(|record| record.user_id.clone())
            .collect();
        assert_eq!(ids, ["me", "ann", "bob"]);
    }
}
