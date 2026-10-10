use serde_json::{Value, json};

pub const DEFAULT_SKIN_TONE: u8 = 2;
const REACTION_TYPE: &str = "Reaction";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reaction {
    Like,
    Heart,
    Applause,
    Laugh,
    Surprised,
}

impl Reaction {
    pub const ALL: [Reaction; 5] = [Reaction::Like, Reaction::Heart, Reaction::Applause, Reaction::Laugh, Reaction::Surprised];

    pub fn name(self) -> &'static str {
        match self {
            Reaction::Like => "like",
            Reaction::Heart => "heart",
            Reaction::Applause => "applause",
            Reaction::Laugh => "laugh",
            Reaction::Surprised => "surprised",
        }
    }

    pub fn from_name(name: &str) -> Option<Reaction> {
        Reaction::ALL.into_iter().find(|reaction| reaction.name() == name)
    }

    fn takes_skin_tone(self) -> bool {
        matches!(self, Reaction::Like | Reaction::Applause)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReactionEvent {
    pub mri: String,
    pub reaction: Reaction,
    pub count: u32,
}

pub fn reaction_message(from: Value, reaction: Reaction, operation_id: &str, value_id: &str) -> Value {
    let skin_tone = reaction.takes_skin_tone().then_some(DEFAULT_SKIN_TONE);
    json!({
        "from": from,
        "messageContent": [{
            "operationId": operation_id,
            "type": REACTION_TYPE,
            "scope": "all",
            "to": [],
            "payload": {"values": [{
                "id": value_id,
                "name": reaction.name(),
                "count": 1,
                "attributes": {"skinTone": skin_tone},
            }]},
        }],
    })
}

pub fn parse_reactions(body: &Value) -> Vec<ReactionEvent> {
    let items: Vec<&Value> = match body["messageContent"].as_array() {
        Some(items) => items.iter().collect(),
        None => vec![body],
    };
    items
        .into_iter()
        .filter(|item| item["type"].as_str() == Some(REACTION_TYPE))
        .flat_map(|item| {
            let mri = item["from"]["id"].as_str().or_else(|| body["from"]["id"].as_str()).unwrap_or_default();
            item["payload"]["values"].as_array().into_iter().flatten().filter_map(move |value| {
                Some(ReactionEvent {
                    mri: mri.to_owned(),
                    reaction: Reaction::from_name(value["name"].as_str()?)?,
                    count: value["count"].as_u64().map_or(1, |count| u32::try_from(count).unwrap_or(u32::MAX)),
                })
            })
        })
        .filter(|event| !event.mri.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_send_body_has_one_reaction_value_with_the_skin_tone_only_where_it_applies() {
        let from = json!({"id": "8:orgid:me"});
        let like = reaction_message(from.clone(), Reaction::Like, "op-1", "val-1");
        let item = &like["messageContent"][0];
        assert_eq!(like["from"]["id"], "8:orgid:me");
        assert_eq!(item["type"], "Reaction");
        assert_eq!(item["scope"], "all");
        assert_eq!(item["operationId"], "op-1");
        assert_eq!(item["payload"]["values"][0]["name"], "like");
        assert_eq!(item["payload"]["values"][0]["count"], 1);
        assert_eq!(item["payload"]["values"][0]["attributes"]["skinTone"], DEFAULT_SKIN_TONE);
        let heart = reaction_message(from, Reaction::Heart, "op-2", "val-2");
        assert!(heart["messageContent"][0]["payload"]["values"][0]["attributes"]["skinTone"].is_null());
    }

    #[test]
    fn every_reaction_name_round_trips() {
        for reaction in Reaction::ALL {
            assert_eq!(Reaction::from_name(reaction.name()), Some(reaction));
        }
        assert_eq!(Reaction::from_name("confetti"), None);
    }

    #[test]
    fn a_single_received_reaction_is_parsed() {
        let body = json!({
            "from": {"id": "8:orgid:a", "displayName": "Ana"},
            "type": "Reaction",
            "payload": {"values": [{"name": "applause", "count": 2, "attributes": {"skinTone": "2"}}]},
            "operationId": "op",
        });
        assert_eq!(parse_reactions(&body), vec![ReactionEvent { mri: "8:orgid:a".into(), reaction: Reaction::Applause, count: 2 }]);
    }

    #[test]
    fn several_message_contents_and_foreign_types_are_handled() {
        let body = json!({
            "from": {"id": "8:orgid:a"},
            "messageContent": [
                {"type": "Reaction", "payload": {"values": [{"name": "heart"}, {"name": "unknown"}]}},
                {"type": "Typing", "payload": {"values": [{"name": "like"}]}},
                {"type": "Reaction", "from": {"id": "8:orgid:b"}, "payload": {"values": [{"name": "laugh", "count": 1}]}},
            ],
        });
        let events = parse_reactions(&body);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], ReactionEvent { mri: "8:orgid:a".into(), reaction: Reaction::Heart, count: 1 });
        assert_eq!(events[1].mri, "8:orgid:b");
    }

    #[test]
    fn a_message_without_a_sender_is_ignored() {
        let body = json!({"type": "Reaction", "payload": {"values": [{"name": "like"}]}});
        assert!(parse_reactions(&body).is_empty());
    }
}
