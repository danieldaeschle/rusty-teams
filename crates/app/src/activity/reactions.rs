use store::MessageRecord;

use super::feed::{ReactedMessage, Sighting};
use super::preview_label;
use crate::notify::preview_of;
use crate::reaction_model::UNKNOWN_REACTOR;
use crate::rows::reaction_glyph;

pub fn reacted_messages(
    records: &[MessageRecord],
    my_user_id: &str,
    name_of: impl Fn(&str) -> Option<String>,
) -> Vec<ReactedMessage> {
    records
        .iter()
        .filter(|record| !record.deleted && record.sender_id.as_deref() == Some(my_user_id))
        .filter_map(|record| {
            let sightings: Vec<Sighting> = teams_core::reactions(record)
                .into_iter()
                .map(|reaction| Sighting {
                    name: reaction
                        .user_name
                        .or_else(|| reaction.user_id.as_deref().and_then(&name_of))
                        .unwrap_or_else(|| UNKNOWN_REACTOR.to_owned()),
                    glyph: reaction_glyph(&reaction.reaction_type),
                    user_id: reaction.user_id,
                    created_at: reaction.created_at,
                })
                .collect();
            (!sightings.is_empty()).then(|| ReactedMessage {
                message_id: record.message_id.clone(),
                preview: preview_label(&preview_of(record)),
                sightings,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;
    use crate::app_state::{AppState, Mode};
    use crate::people::resolve_names;

    fn record(message_id: &str, sender: &str, reactions_json: &str) -> MessageRecord {
        MessageRecord {
            conversation_id: "chat".into(),
            message_id: message_id.into(),
            sender_id: Some(sender.into()),
            created_at: Utc.with_ymd_and_hms(2026, 10, 8, 9, 0, 0).unwrap(),
            body_html: "<p>hello there</p>".into(),
            attachments_json: "[]".into(),
            reactions_json: reactions_json.into(),
            mentions_json: "[]".into(),
            ..Default::default()
        }
    }

    #[test]
    fn only_own_reacted_messages_are_returned() {
        let reaction =
            r#"[{"reaction_type":"like","user_id":"u1","user_name":null,"created_at":null}]"#;
        let records = vec![
            record("mine", "me", reaction),
            record("theirs", "u2", reaction),
            record("quiet", "me", "[]"),
        ];
        let found = reacted_messages(&records, "me", |user_id| {
            (user_id == "u1").then(|| "Anna".to_owned())
        });
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].message_id, "mine");
        assert_eq!(found[0].preview, "hello there");
        assert_eq!(found[0].sightings[0].name, "Anna");
        assert_eq!(found[0].sightings[0].glyph, "\u{1F44D}");
    }

    #[test]
    fn unknown_reactor_gets_the_placeholder_name() {
        let reaction = r#"[{"reaction_type":"heart","user_id":"u9"}]"#;
        let found = reacted_messages(&[record("mine", "me", reaction)], "me", |_| None);
        assert_eq!(found[0].sightings[0].name, UNKNOWN_REACTOR);
    }

    #[test]
    fn channel_reactor_is_named_from_another_message_sender() {
        let reaction = r#"[{"reaction_type":"like","user_id":"u5"}]"#;
        let mut other = record("other", "u5", "[]");
        other.sender_name = Some("Cleo".into());
        let records = vec![record("mine", "me", reaction), other];
        let store = std::sync::Arc::new(store::Store::open_in_memory().unwrap());
        store.upsert_messages(&records).unwrap();
        let state = AppState::new(store, Mode::default());
        let names = resolve_names(&state, &["u5".to_owned(), "u6".to_owned()]);
        let found = reacted_messages(&records, "me", |user_id| names.get(user_id).cloned());
        assert_eq!(found[0].sightings[0].name, "Cleo");
        assert!(!names.contains_key("u6"));
    }
}
