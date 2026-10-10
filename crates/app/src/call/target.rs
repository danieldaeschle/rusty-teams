use calling::meeting::LiveMeeting;
use store::ChatRecord;

use crate::data::{Person, is_one_on_one, others};

const ORGID_PREFIX: &str = "8:orgid:";
const GROUP_KIND: &str = "group";

pub struct CallPlan {
    pub title: String,
    pub callees: Vec<(String, String)>,
}

pub fn plan_for_chat(chat: &ChatRecord, me: Option<&Person>) -> Option<CallPlan> {
    if !is_one_on_one(chat) && !chat.kind.eq_ignore_ascii_case(GROUP_KIND) {
        return None;
    }
    let callees: Vec<(String, String)> = others(chat, me)
        .into_iter()
        .filter_map(|(user_id, name)| Some((format!("{ORGID_PREFIX}{}", user_id?), name)))
        .collect();
    let title = match callees.as_slice() {
        [] => return None,
        [(_, name)] if is_one_on_one(chat) => name.clone(),
        _ => crate::app_state::chat_title(chat),
    };
    Some(CallPlan { title, callees })
}

pub fn is_organizer(meeting: &LiveMeeting, me: Option<&Person>) -> bool {
    match (meeting.organizer_id.as_deref(), me) {
        (Some(organizer), Some(me)) => organizer.eq_ignore_ascii_case(&me.user_id),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use store::MemberRecord;

    use super::*;

    fn chat(kind: &str, title: &str, members: &[(Option<&str>, &str)]) -> ChatRecord {
        ChatRecord {
            id: "19:chat".into(),
            kind: kind.into(),
            title: title.into(),
            members: members
                .iter()
                .map(|(user_id, name)| MemberRecord {
                    user_id: user_id.map(str::to_owned),
                    display_name: (*name).to_owned(),
                })
                .collect(),
            ..Default::default()
        }
    }

    fn me() -> Person {
        Person {
            user_id: "me-id".into(),
            display_name: "Me".into(),
        }
    }

    #[test]
    fn a_one_on_one_chat_calls_the_other_person_by_name() {
        let chat = chat("oneOnOne", "", &[(Some("me-id"), "Me"), (Some("bea-id"), "Bea")]);
        let plan = plan_for_chat(&chat, Some(&me())).unwrap();
        assert_eq!(plan.title, "Bea");
        assert_eq!(plan.callees, vec![("8:orgid:bea-id".to_owned(), "Bea".to_owned())]);
    }

    #[test]
    fn a_group_chat_calls_everybody_else_under_the_chat_title() {
        let chat = chat(
            "group",
            "Retro team",
            &[(Some("me-id"), "Me"), (Some("bea-id"), "Bea"), (Some("cy-id"), "Cy")],
        );
        let plan = plan_for_chat(&chat, Some(&me())).unwrap();
        assert_eq!(plan.title, "Retro team");
        assert_eq!(plan.callees.len(), 2);
    }

    #[test]
    fn meeting_chats_and_chats_without_callable_members_have_no_call_button() {
        let meeting = chat("meeting", "Standup", &[(Some("bea-id"), "Bea")]);
        assert!(plan_for_chat(&meeting, Some(&me())).is_none());
        let alone = chat("oneOnOne", "Notes", &[(Some("me-id"), "Me")]);
        assert!(plan_for_chat(&alone, Some(&me())).is_none());
        let unknown_ids = chat("group", "Guests", &[(None, "Guest")]);
        assert!(plan_for_chat(&unknown_ids, Some(&me())).is_none());
    }

    #[test]
    fn the_organizer_may_end_the_meeting() {
        let meeting = LiveMeeting {
            thread_id: "t".into(),
            conversation_url: None,
            expiration: None,
            organizer_id: Some("ME-ID".into()),
            tenant_id: None,
            meeting_code: None,
            passcode: None,
        };
        assert!(is_organizer(&meeting, Some(&me())));
        let other = LiveMeeting {
            organizer_id: Some("someone".into()),
            ..meeting.clone()
        };
        assert!(!is_organizer(&other, Some(&me())));
        assert!(!is_organizer(&meeting, None));
    }
}
