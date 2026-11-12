use std::collections::{HashMap, HashSet};

use chrono::{FixedOffset, NaiveDate};
use teams_core::substrate_search::{SearchMessage, SearchPerson, split_highlights};

use super::switcher::MessageResult;
use crate::app_state::Selection;
use crate::format;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonResult {
    pub user_id: String,
    pub name: String,
    pub subtitle: String,
}

pub fn people_results(
    people: &[SearchPerson],
    shown_titles: &HashSet<String>,
    my_user_id: Option<&str>,
    limit: usize,
) -> Vec<PersonResult> {
    people
        .iter()
        .filter_map(|person| {
            let user_id = person.user_id.clone()?;
            let subtitle = [person.job_title.as_deref(), person.department.as_deref()]
                .into_iter()
                .flatten()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(", ");
            Some(PersonResult {
                user_id,
                name: person.display_name.clone(),
                subtitle,
            })
        })
        .filter(|person| Some(person.user_id.as_str()) != my_user_id)
        .filter(|person| !shown_titles.contains(&person.name))
        .take(limit)
        .collect()
}

pub fn server_message_results(
    messages: &[SearchMessage],
    conversations: &HashMap<String, (String, Selection)>,
    today: NaiveDate,
    offset: FixedOffset,
) -> Vec<MessageResult> {
    messages
        .iter()
        .filter_map(|message| {
            let (conversation, selection) = conversations.get(&message.conversation_id)?.clone();
            let highlighted = split_highlights(
                message
                    .highlighted_summary
                    .as_deref()
                    .unwrap_or(&message.preview),
            );
            Some(MessageResult {
                selection,
                message_id: message.message_id.clone(),
                author: message
                    .sender_name
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_owned()),
                conversation,
                date: message
                    .sent_at
                    .map(|sent_at| format::list_time_label(sent_at, today, offset))
                    .unwrap_or_default(),
                snippet: highlighted.text.replace(['\n', '\r'], " "),
                matches: highlighted.hits,
            })
        })
        .collect()
}

pub fn merge_messages(
    local: &[MessageResult],
    server: Vec<MessageResult>,
    limit: usize,
) -> Vec<MessageResult> {
    let mut seen: HashSet<String> = HashSet::new();
    local
        .iter()
        .cloned()
        .chain(server)
        .filter(|message| seen.insert(message.message_id.clone()))
        .take(limit)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(user_id: &str, name: &str) -> SearchPerson {
        SearchPerson {
            reference_id: None,
            user_id: Some(user_id.to_owned()),
            mri: None,
            display_name: name.to_owned(),
            given_name: None,
            surname: None,
            email_addresses: Vec::new(),
            user_principal_name: None,
            job_title: Some("Engineer".to_owned()),
            department: Some("AI".to_owned()),
            company_name: None,
            office_location: None,
            people_subtype: None,
        }
    }

    fn message(id: &str, conversation_id: &str) -> MessageResult {
        MessageResult {
            selection: Selection::Chat(conversation_id.to_owned()),
            message_id: id.to_owned(),
            author: "Ada".to_owned(),
            conversation: "Chat".to_owned(),
            date: String::new(),
            snippet: String::new(),
            matches: Vec::new(),
        }
    }

    #[test]
    fn people_skip_me_shown_chats_and_entries_without_an_id() {
        let mut nameless = person("x", "No Id");
        nameless.user_id = None;
        let people = [
            person("me", "Me Myself"),
            person("a", "Ada Lovelace"),
            person("b", "Grace Hopper"),
            nameless,
        ];
        let shown = HashSet::from(["Ada Lovelace".to_owned()]);
        let results = people_results(&people, &shown, Some("me"), 5);
        assert_eq!(
            results,
            [PersonResult {
                user_id: "b".to_owned(),
                name: "Grace Hopper".to_owned(),
                subtitle: "Engineer, AI".to_owned(),
            }]
        );
    }

    #[test]
    fn server_messages_need_a_known_conversation_and_keep_highlights() {
        let conversations = HashMap::from([(
            "19:a".to_owned(),
            ("Ada".to_owned(), Selection::Chat("19:a".to_owned())),
        )]);
        let hit = |conversation_id: &str| SearchMessage {
            conversation_id: conversation_id.to_owned(),
            message_id: "1".to_owned(),
            highlighted_summary: Some("the <c0>build</c0>\nis green".to_owned()),
            ..SearchMessage::default()
        };
        let today = NaiveDate::from_ymd_opt(2026, 11, 8).unwrap();
        let offset = FixedOffset::east_opt(0).unwrap();
        let results = server_message_results(
            &[hit("19:a"), hit("19:unknown")],
            &conversations,
            today,
            offset,
        );
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].snippet, "the build is green");
        assert_eq!(results[0].matches, [4..9]);
    }

    #[test]
    fn merged_messages_keep_local_first_without_duplicates() {
        let merged = merge_messages(
            &[message("1", "19:a")],
            vec![
                message("1", "19:a"),
                message("2", "19:a"),
                message("3", "19:a"),
            ],
            2,
        );
        let ids: Vec<&str> = merged
            .iter()
            .map(|message| message.message_id.as_str())
            .collect();
        assert_eq!(ids, ["1", "2"]);
    }
}
