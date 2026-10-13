use chrono::{DateTime, TimeZone, Utc};
use store::{
    AgentProposalMention, AgentProposalRecord, AgentReactionRecord, AttachmentImage, MessageRecord,
    Store,
};

fn at(minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 6, 9, minute, 0).unwrap()
}

fn message(conversation_id: &str, message_id: &str, minute: u32, html: &str) -> MessageRecord {
    MessageRecord {
        conversation_id: conversation_id.to_owned(),
        message_id: message_id.to_owned(),
        sender_id: Some("user-ada".to_owned()),
        sender_name: Some("Ada Example".to_owned()),
        created_at: at(minute),
        body_html: html.to_owned(),
        attachments_json: "[]".to_owned(),
        reactions_json: "[]".to_owned(),
        mentions_json: "[]".to_owned(),
        links_json: "[]".to_owned(),
        ..MessageRecord::default()
    }
}

fn proposal(id: &str, conversation_id: &str, minute: u32) -> AgentProposalRecord {
    AgentProposalRecord {
        id: id.to_owned(),
        conversation_id: conversation_id.to_owned(),
        reply_to_message_id: None,
        text: format!("text {id}"),
        mentions: Vec::new(),
        images: Vec::new(),
        agent: Some("claude".to_owned()),
        session_id: None,
        created_at: at(minute),
        state: "pending".to_owned(),
        resolved_at: None,
    }
}

#[test]
fn search_in_filters_before_the_limit_applies() {
    let store = Store::open_in_memory().unwrap();
    store
        .upsert_messages(&[
            message("c1", "m1", 1, "<p>budget one</p>"),
            message("c2", "m2", 2, "<p>budget two</p>"),
            message("c2", "m3", 3, "<p>budget three</p>"),
        ])
        .unwrap();
    let hits = store
        .search_messages_in("budget", &["c1".to_owned()], 1)
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].message_id, "m1");
    assert!(
        store
            .search_messages_in("budget", &[], 10)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn messages_after_pages_forward_oldest_first() {
    let store = Store::open_in_memory().unwrap();
    store
        .upsert_messages(&[
            message("c1", "m1", 1, "<p>a</p>"),
            message("c1", "m2", 2, "<p>b</p>"),
            message("c1", "m3", 3, "<p>c</p>"),
            message("c2", "m4", 4, "<p>d</p>"),
        ])
        .unwrap();
    let page = store.messages_after("c1", at(1), 1).unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].message_id, "m2");
    assert_eq!(store.messages_after("c1", at(1), 10).unwrap().len(), 2);
}

#[test]
fn proposals_persist_and_only_pending_ones_are_listed() {
    let store = Store::open_in_memory().unwrap();
    store
        .insert_agent_proposal(&proposal("p2", "c1", 2))
        .unwrap();
    store
        .insert_agent_proposal(&proposal("p1", "c2", 1))
        .unwrap();
    let pending = store.pending_agent_proposals().unwrap();
    assert_eq!(
        pending
            .iter()
            .map(|record| record.id.as_str())
            .collect::<Vec<_>>(),
        ["p1", "p2"]
    );
    assert_eq!(pending[0], proposal("p1", "c2", 1));

    assert!(
        store
            .set_agent_proposal_state("p1", "sent", Some(at(5)))
            .unwrap()
    );
    assert!(
        !store
            .set_agent_proposal_state("nope", "sent", None)
            .unwrap()
    );
    let pending = store.pending_agent_proposals().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, "p2");
}

#[test]
fn proposal_mentions_and_images_survive_a_restart_and_images_go_when_resolved() {
    let store = Store::open_in_memory().unwrap();
    let image = AttachmentImage {
        name: "image-1.png".to_owned(),
        format: "image/png".to_owned(),
        bytes: vec![1, 2, 3],
        width: None,
        height: None,
    };
    let rich = AgentProposalRecord {
        mentions: vec![AgentProposalMention {
            user_id: "u1".to_owned(),
            name: "Ada Example".to_owned(),
        }],
        images: vec![image],
        ..proposal("p1", "c1", 1)
    };
    store.insert_agent_proposal(&rich).unwrap();
    assert_eq!(store.pending_agent_proposals().unwrap(), [rich.clone()]);

    store
        .set_agent_proposal_state("p1", "sent", Some(at(2)))
        .unwrap();
    store
        .insert_agent_proposal(&AgentProposalRecord {
            id: "p1".to_owned() + "-again",
            ..rich
        })
        .unwrap();
    let pending = store.pending_agent_proposals().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].images.len(), 1);
    assert_eq!(pending[0].mentions.len(), 1);
}

fn reaction(id: &str, message_id: &str, minute: u32) -> AgentReactionRecord {
    AgentReactionRecord {
        id: id.to_owned(),
        conversation_id: "c1".to_owned(),
        message_id: message_id.to_owned(),
        emoji: "\u{1F44D}".to_owned(),
        agent: None,
        session_id: None,
        created_at: at(minute),
        state: "pending".to_owned(),
        resolved_at: None,
    }
}

#[test]
fn reaction_proposals_persist_until_resolved() {
    let store = Store::open_in_memory().unwrap();
    store
        .insert_agent_reaction(&reaction("r2", "m2", 2))
        .unwrap();
    store
        .insert_agent_reaction(&reaction("r1", "m1", 1))
        .unwrap();
    let pending = store.pending_agent_reactions().unwrap();
    assert_eq!(pending[0], reaction("r1", "m1", 1));
    assert_eq!(pending.len(), 2);
    assert!(
        store
            .set_agent_reaction_state("r1", "accepted", Some(at(3)))
            .unwrap()
    );
    let pending = store.pending_agent_reactions().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, "r2");
}

#[test]
fn reaction_state_lists_only_the_senders_undeleted_messages_newest_first() {
    let store = Store::open_in_memory().unwrap();
    let own = |id: &str, minute: u32, reactions: &str| MessageRecord {
        sender_id: Some("me".to_owned()),
        reactions_json: reactions.to_owned(),
        ..message("c1", id, minute, "<p>x</p>")
    };
    let mut deleted = own("m3", 3, "[]");
    deleted.deleted = true;
    store
        .upsert_messages(&[
            own("m1", 1, "[1]"),
            own("m2", 2, "[2]"),
            deleted,
            message("c1", "m4", 4, "<p>other</p>"),
        ])
        .unwrap();
    let state = store.reaction_state_of_sender("c1", "me", 10).unwrap();
    assert_eq!(
        state,
        [
            ("m2".to_owned(), "[2]".to_owned()),
            ("m1".to_owned(), "[1]".to_owned())
        ]
    );
    assert_eq!(
        store.reaction_state_of_sender("c1", "me", 1).unwrap().len(),
        1
    );
}

#[test]
fn conversation_people_reads_exact_mention_ids_and_survives_bad_json() {
    let store = Store::open_in_memory().unwrap();
    let with_mentions = |id: &str, minute: u32, json: &str| MessageRecord {
        sender_id: None,
        sender_name: None,
        mentions_json: json.to_owned(),
        ..message("c1", id, minute, "<p>hi</p>")
    };
    store
        .upsert_messages(&[
            with_mentions("m1", 1, r#"[{"user_id":"user-bobby","name":"Bobby"}]"#),
            with_mentions("m2", 2, "not json"),
            with_mentions("m3", 3, r#"[{"user_id":"user-cy"}]"#),
        ])
        .unwrap();
    assert_eq!(
        store.conversation_people("c1", 100).unwrap(),
        [("user-bobby".to_owned(), "Bobby".to_owned())]
    );
}

#[test]
fn conversation_people_lists_senders_and_mentioned_names_once() {
    let store = Store::open_in_memory().unwrap();
    let mentioning = MessageRecord {
        mentions_json: r#"[{"user_id":"user-bob","name":"Bob Sample"},{"user_id":"user-ada","name":"Ada Example"}]"#.to_owned(),
        ..message("c1", "m2", 2, "<p>hi</p>")
    };
    store
        .upsert_messages(&[message("c1", "m1", 1, "<p>hi</p>"), mentioning])
        .unwrap();
    let mut people = store.conversation_people("c1", 100).unwrap();
    people.sort();
    assert_eq!(
        people,
        [
            ("user-ada".to_owned(), "Ada Example".to_owned()),
            ("user-bob".to_owned(), "Bob Sample".to_owned())
        ]
    );
    assert!(store.conversation_people("c2", 100).unwrap().is_empty());
}

#[test]
fn conversation_people_only_scans_the_newest_messages() {
    let store = Store::open_in_memory().unwrap();
    let sender = |id: &str, minute: u32, user: &str| MessageRecord {
        sender_id: Some(user.to_owned()),
        sender_name: Some(user.to_owned()),
        ..message("c1", id, minute, "<p>x</p>")
    };
    store
        .upsert_messages(&[sender("m1", 1, "old"), sender("m2", 2, "new")])
        .unwrap();
    assert_eq!(
        store.conversation_people("c1", 1).unwrap(),
        [("new".to_owned(), "new".to_owned())]
    );
}
