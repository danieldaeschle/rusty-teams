use chrono::{DateTime, TimeZone, Utc};
use store::{AgentProposalRecord, MessageRecord, Store};

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
