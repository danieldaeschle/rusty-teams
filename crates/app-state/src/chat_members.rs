use std::cmp::Ordering;

use store::ChatRecord;

use crate::data::{Person, is_one_on_one};

const MIN_MEMBERS_FOR_REMOVAL: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberRow {
    pub user_id: Option<String>,
    pub name: String,
    pub is_self: bool,
    pub removable: bool,
}

pub fn member_rows(chat: &ChatRecord, me: Option<&Person>) -> Vec<MemberRow> {
    let my_user_id = me.map(|person| person.user_id.as_str());
    let removal_allowed = can_change_members(chat, my_user_id);
    let mut rows: Vec<MemberRow> = chat
        .members
        .iter()
        .map(|member| {
            let is_self = member.user_id.is_some() && member.user_id.as_deref() == my_user_id;
            MemberRow {
                user_id: member.user_id.clone(),
                name: member.display_name.clone(),
                is_self,
                removable: removal_allowed && !is_self && member.user_id.is_some(),
            }
        })
        .collect();
    rows.sort_by(|left, right| compare_names(&left.name, &right.name));
    rows
}

pub fn can_leave(chat: &ChatRecord, me: Option<&Person>) -> bool {
    can_change_members(chat, me.map(|person| person.user_id.as_str()))
}

fn can_change_members(chat: &ChatRecord, my_user_id: Option<&str>) -> bool {
    let is_member = my_user_id.is_some_and(|user_id| {
        chat.members
            .iter()
            .any(|member| member.user_id.as_deref() == Some(user_id))
    });
    is_member && !is_one_on_one(chat) && chat.members.len() >= MIN_MEMBERS_FOR_REMOVAL
}

fn compare_names(left: &str, right: &str) -> Ordering {
    left.to_lowercase()
        .cmp(&right.to_lowercase())
        .then_with(|| left.cmp(right))
}

#[cfg(test)]
mod tests {
    use store::MemberRecord;

    use super::*;

    fn member(user_id: &str, name: &str) -> MemberRecord {
        MemberRecord {
            user_id: Some(user_id.to_owned()),
            display_name: name.to_owned(),
        }
    }

    fn me() -> Person {
        Person {
            user_id: "me".to_owned(),
            display_name: "Daniel".to_owned(),
        }
    }

    fn group(members: Vec<MemberRecord>) -> ChatRecord {
        ChatRecord {
            id: "19:group@thread.v2".to_owned(),
            kind: "group".to_owned(),
            members,
            ..ChatRecord::default()
        }
    }

    #[test]
    fn rows_are_sorted_by_name_ignoring_case_and_include_self() {
        let chat = group(vec![
            member("u2", "zoe"),
            member("me", "Daniel"),
            member("u1", "Anna"),
        ]);
        let names: Vec<String> = member_rows(&chat, Some(&me()))
            .into_iter()
            .map(|row| row.name)
            .collect();
        assert_eq!(names, ["Anna", "Daniel", "zoe"]);
    }

    #[test]
    fn others_are_removable_in_a_group_of_three() {
        let chat = group(vec![
            member("me", "Daniel"),
            member("u1", "Anna"),
            member("u2", "Zoe"),
        ]);
        let rows = member_rows(&chat, Some(&me()));
        let removable: Vec<(&str, bool)> = rows
            .iter()
            .map(|row| (row.name.as_str(), row.removable))
            .collect();
        assert_eq!(
            removable,
            [("Anna", true), ("Daniel", false), ("Zoe", true)]
        );
        assert!(rows.iter().any(|row| row.is_self && row.name == "Daniel"));
        assert!(can_leave(&chat, Some(&me())));
    }

    #[test]
    fn nobody_is_removable_below_three_members_or_when_not_a_member() {
        let pair = group(vec![member("me", "Daniel"), member("u1", "Anna")]);
        assert!(
            member_rows(&pair, Some(&me()))
                .iter()
                .all(|row| !row.removable)
        );
        assert!(!can_leave(&pair, Some(&me())));
        let outside = group(vec![
            member("u1", "Anna"),
            member("u2", "Zoe"),
            member("u3", "Max"),
        ]);
        assert!(
            member_rows(&outside, Some(&me()))
                .iter()
                .all(|row| !row.removable)
        );
        assert!(!can_leave(&outside, Some(&me())));
    }
}
