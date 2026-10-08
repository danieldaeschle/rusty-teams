use graph::{Chat, Message, User};
use serde_json::Value;

fn fixture(name: &str) -> Value {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn items<T: serde::de::DeserializeOwned>(name: &str) -> Vec<T> {
    serde_json::from_value(fixture(name)["value"].clone()).unwrap()
}

#[test]
fn parses_user() {
    let user: User = serde_json::from_str(
        r#"{"@odata.context":"x","id":"user-me","displayName":"Test Me","mail":null,"userPrincipalName":"me@example.test"}"#,
    )
    .unwrap();
    assert_eq!(user.id, "user-me");
    assert!(user.mail.is_none());
}

#[test]
fn chat_titles_follow_topic_then_other_members() {
    let chats: Vec<Chat> = items("chats.json");
    let titles: Vec<String> = chats.iter().map(|chat| chat.title("user-me")).collect();
    assert_eq!(
        titles,
        [
            "Ada Example",
            "Project Sample",
            "cy@example.test, Di Sample",
            "(only you)"
        ]
    );
}

fn solo_chat(sender: Value) -> Chat {
    serde_json::from_value(serde_json::json!({
        "id": "19:solo@unq.gbl.spaces",
        "chatType": "oneOnOne",
        "members": [{"userId": "user-me", "displayName": "Test Me"}],
        "lastMessagePreview": {"id": "1", "from": sender}
    }))
    .unwrap()
}

#[test]
fn chat_with_only_me_is_named_after_the_bot_or_user_of_the_last_message() {
    let bot =
        solo_chat(serde_json::json!({"application": {"id": "app-1", "displayName": "Sample Bot"}}));
    assert_eq!(bot.title("user-me"), "Sample Bot");
    let other =
        solo_chat(serde_json::json!({"user": {"id": "user-ada", "displayName": "Ada Example"}}));
    assert_eq!(other.title("user-me"), "Ada Example");
}

#[test]
fn chat_with_only_me_stays_a_note_to_self_without_another_sender() {
    let mine = solo_chat(serde_json::json!({"user": {"id": "user-me", "displayName": "Test Me"}}));
    assert_eq!(mine.title("user-me"), "(only you)");
    let nameless = solo_chat(serde_json::json!({"application": {"id": "app-1"}}));
    assert_eq!(nameless.title("user-me"), "(only you)");
}

#[test]
fn chat_last_message_time_comes_from_preview() {
    let chats: Vec<Chat> = items("chats.json");
    assert_eq!(
        chats[0].last_message_time().unwrap().to_rfc3339(),
        "2026-10-06T08:15:00.123+00:00"
    );
    assert!(chats[1].last_message_time().is_none());
    assert!(chats[2].last_message_time().is_none());
}

#[test]
fn chat_viewpoint_is_optional() {
    let chats: Vec<Chat> = items("chats.json");
    assert!(
        chats[0]
            .viewpoint
            .as_ref()
            .unwrap()
            .last_message_read_date_time
            .is_some()
    );
    assert!(chats[1].viewpoint.is_none());
}

#[test]
fn parses_messages_with_edits_reactions_and_deletes() {
    let messages: Vec<Message> = items("messages.json");
    assert_eq!(messages[0].attachments[0].name.as_deref(), Some("a.pdf"));
    assert_eq!(messages[0].reactions[0].reaction_type, "like");
    assert!(messages[0].last_edited_date_time.is_some());
    assert_eq!(
        messages[0].body.as_ref().unwrap().content.as_deref(),
        Some("<p>edited <b>text</b></p>")
    );
    assert!(messages[1].from.is_none());
    assert!(messages[2].is_deleted());
    assert_eq!(
        messages[2].from.as_ref().unwrap().display_name(),
        Some("Build Bot")
    );
    assert_eq!(messages[2].from.as_ref().unwrap().user_id(), None);
}

#[test]
fn parses_channel_thread_with_replies() {
    let threads: Vec<Message> = items("channel_messages.json");
    assert_eq!(threads[0].subject.as_deref(), Some("Release notes"));
    assert_eq!(threads[0].replies.len(), 1);
    assert_eq!(
        threads[0].replies[0].reply_to_id.as_deref(),
        Some("1800000000001")
    );
}

#[test]
fn a_tag_mention_exposes_its_target_id_and_name() {
    let message: Message = serde_json::from_str(
        r#"{"id":"1","mentions":[{"id":0,"mentionText":"Tip","mentioned":{"tag":{"id":"T1","displayName":"Tip of the day"}}}]}"#,
    )
    .unwrap();
    let mentioned = message.mentions[0].mentioned.as_ref().unwrap();
    assert_eq!(mentioned.target_id(), Some("T1"));
    assert_eq!(mentioned.display_name(), Some("Tip of the day"));
    assert_eq!(mentioned.user_id(), None);
}
