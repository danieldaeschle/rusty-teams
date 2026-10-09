use chrono::{DateTime, TimeZone, Utc};
use store::{AttachmentImage, DraftRecord, OutboxRecord, OutboxState, OutboxTarget, Store};

fn at(second: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 6, 9, 0, second).unwrap()
}

fn image(name: &str) -> AttachmentImage {
    AttachmentImage {
        name: name.to_owned(),
        format: "image/png".to_owned(),
        bytes: vec![1, 2, 3, 4],
        width: Some(10),
        height: None,
    }
}

fn outbox(id: &str, conversation_id: &str, second: u32) -> OutboxRecord {
    OutboxRecord {
        id: id.to_owned(),
        conversation_id: conversation_id.to_owned(),
        target: OutboxTarget::Flat,
        thread_root_id: None,
        payload: format!("{{\"id\":\"{id}\"}}"),
        images: vec![image("a.png"), image("b.png")],
        state: OutboxState::Sending,
        last_error: None,
        created_at: at(second),
    }
}

fn draft(conversation_id: &str, preview: &str) -> DraftRecord {
    DraftRecord {
        conversation_id: conversation_id.to_owned(),
        payload: "{}".to_owned(),
        preview: preview.to_owned(),
        images: vec![image("pasted.png")],
        updated_at: at(1),
    }
}

#[test]
fn outbox_rows_round_trip_with_images_oldest_first() {
    let store = Store::open_in_memory().unwrap();
    let mut thread = outbox("late", "chat-1", 30);
    thread.target = OutboxTarget::Thread;
    thread.thread_root_id = Some("root-1".to_owned());
    store.put_outbox(&thread).unwrap();
    store.put_outbox(&outbox("early", "chat-1", 10)).unwrap();
    store.put_outbox(&outbox("other", "chat-2", 20)).unwrap();
    let rows = store.outbox_for_conversation("chat-1").unwrap();
    assert_eq!(rows, vec![outbox("early", "chat-1", 10), thread]);
}

#[test]
fn failed_rows_leave_the_sending_list_and_keep_their_error() {
    let store = Store::open_in_memory().unwrap();
    store.put_outbox(&outbox("a", "chat-1", 10)).unwrap();
    store.put_outbox(&outbox("b", "chat-2", 20)).unwrap();
    store.mark_outbox_failed("a", "HTTP 403").unwrap();
    let sending = store.sending_outbox().unwrap();
    assert_eq!(sending.len(), 1);
    assert_eq!(sending[0].id, "b");
    let failed = store.outbox_for_conversation("chat-1").unwrap();
    assert_eq!(failed[0].state, OutboxState::Failed);
    assert_eq!(failed[0].last_error.as_deref(), Some("HTTP 403"));
    assert_eq!(store.failed_outbox_conversations().unwrap(), vec!["chat-1"]);
}

#[test]
fn putting_a_failed_row_again_makes_it_a_fresh_attempt() {
    let store = Store::open_in_memory().unwrap();
    store.put_outbox(&outbox("a", "chat-1", 10)).unwrap();
    store.mark_outbox_failed("a", "boom").unwrap();
    store.put_outbox(&outbox("a", "chat-1", 40)).unwrap();
    let rows = store.outbox_for_conversation("chat-1").unwrap();
    assert_eq!(rows, vec![outbox("a", "chat-1", 40)]);
    assert!(store.failed_outbox_conversations().unwrap().is_empty());
}

#[test]
fn deleting_an_outbox_row_removes_its_images() {
    let store = Store::open_in_memory().unwrap();
    store.put_outbox(&outbox("a", "chat-1", 10)).unwrap();
    store.delete_outbox("a").unwrap();
    assert!(store.outbox_for_conversation("chat-1").unwrap().is_empty());
    store.put_outbox(&outbox("a", "chat-1", 10)).unwrap();
    assert_eq!(
        store.outbox_for_conversation("chat-1").unwrap()[0]
            .images
            .len(),
        2
    );
}

#[test]
fn drafts_save_load_replace_and_delete() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(store.draft("chat-1").unwrap(), None);
    store.save_draft(&draft("chat-1", "first")).unwrap();
    store.save_draft(&draft("chat-2", "other")).unwrap();
    let mut changed = draft("chat-1", "second");
    changed.images.clear();
    store.save_draft(&changed).unwrap();
    assert_eq!(store.draft("chat-1").unwrap(), Some(changed));
    assert_eq!(
        store.draft("chat-2").unwrap(),
        Some(draft("chat-2", "other"))
    );
    store.delete_draft("chat-1").unwrap();
    assert_eq!(store.draft("chat-1").unwrap(), None);
}

#[test]
fn draft_previews_map_conversations_to_plain_text() {
    let store = Store::open_in_memory().unwrap();
    store.save_draft(&draft("chat-1", "hello")).unwrap();
    store.save_draft(&draft("chat-2", "world")).unwrap();
    let previews = store.draft_previews().unwrap();
    assert_eq!(previews.len(), 2);
    assert_eq!(previews["chat-1"], "hello");
    assert_eq!(previews["chat-2"], "world");
}

#[test]
fn draft_and_outbox_images_do_not_collide_on_the_same_id() {
    let store = Store::open_in_memory().unwrap();
    store.put_outbox(&outbox("same", "chat-1", 10)).unwrap();
    store.save_draft(&draft("same", "text")).unwrap();
    store.delete_draft("same").unwrap();
    assert_eq!(
        store.outbox_for_conversation("chat-1").unwrap()[0]
            .images
            .len(),
        2
    );
}

#[test]
fn saving_only_the_draft_text_keeps_the_stored_images() {
    let store = Store::open_in_memory().unwrap();
    store.save_draft(&draft("chat-1", "first")).unwrap();
    let mut edited = draft("chat-1", "second");
    edited.payload = "{\"lines\":[]}".to_owned();
    edited.images.clear();
    edited.updated_at = at(9);
    store.save_draft_text(&edited).unwrap();
    let loaded = store.draft("chat-1").unwrap().unwrap();
    assert_eq!(loaded.preview, "second");
    assert_eq!(loaded.payload, "{\"lines\":[]}");
    assert_eq!(loaded.updated_at, at(9));
    assert_eq!(loaded.images, vec![image("pasted.png")]);
}
