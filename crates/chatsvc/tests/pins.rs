use std::collections::VecDeque;
use std::sync::Mutex;

use chatsvc::pins::{CsaTransport, Pins};
use chatsvc::{Error, Result};
use serde_json::{Value, json};
use session::{ApiResponse, Request};

struct Mock {
    answers: Mutex<VecDeque<ApiResponse>>,
    requests: Mutex<Vec<Request>>,
}

impl Mock {
    fn new(answers: Vec<(u16, Value)>) -> Self {
        Mock {
            answers: Mutex::new(
                answers
                    .into_iter()
                    .map(|(status, body)| ApiResponse {
                        status,
                        body,
                        retry_after: None,
                    })
                    .collect(),
            ),
            requests: Mutex::new(Vec::new()),
        }
    }
}

impl CsaTransport for &Mock {
    async fn send(&self, request: Request) -> Result<ApiResponse> {
        self.requests.lock().unwrap().push(request);
        Ok(self
            .answers
            .lock()
            .unwrap()
            .pop_front()
            .expect("mock answers exhausted"))
    }
}

const FOLDER: &str = "t~u~Favorites";

fn folders(version: i64, chat_ids: &[&str]) -> Value {
    let items: Vec<Value> = chat_ids
        .iter()
        .map(|id| json!({"conversationId": id}))
        .collect();
    json!({
        "folderHierarchyVersion": version,
        "conversationFolders": [
            {"id": "t~u~Recent", "folderType": "RecentChats", "conversationFolderItems": []},
            {"id": FOLDER, "name": "Team", "folderType": "Favorites", "conversationFolderItems": items}
        ]
    })
}

fn pins(mock: &Mock) -> Pins<&Mock> {
    Pins::with_transport(mock, "emea")
}

fn sent(mock: &Mock) -> Vec<(String, String, Option<Value>)> {
    mock.requests
        .lock()
        .unwrap()
        .iter()
        .map(|request| {
            (
                request.method.as_str().to_owned(),
                request.url.clone(),
                request.body.clone(),
            )
        })
        .collect()
}

#[tokio::test]
async fn reads_pinned_chats_in_order_from_the_favorites_folder() {
    let mock = Mock::new(vec![(200, folders(10, &["19:a@thread.v2", "48:notes"]))]);
    let state = pins(&mock).pinned_chats().await.unwrap();
    assert_eq!(state.chat_ids, ["19:a@thread.v2", "48:notes"]);
    assert_eq!(state.hierarchy_version, 10);
    let requests = sent(&mock);
    assert_eq!(requests[0].0, "GET");
    assert_eq!(
        requests[0].1,
        "https://teams.cloud.microsoft/api/csa/emea/api/v1/teams/users/me/conversationFolders?supportsAdditionalSystemGeneratedFolders=true&supportsSliceItems=true"
    );
}

#[tokio::test]
async fn region_is_configurable() {
    let mock = Mock::new(vec![(200, folders(1, &[]))]);
    Pins::with_transport(&mock, "amer")
        .pinned_chats()
        .await
        .unwrap();
    assert!(sent(&mock)[0].1.contains("/api/csa/amer/api/v1/"));
}

#[tokio::test]
async fn pin_at_index_sends_add_item_with_the_read_version() {
    let mock = Mock::new(vec![
        (200, folders(10, &["a"])),
        (200, folders(11, &["a", "c", "b"])),
    ]);
    let state = pins(&mock).pin_chat("b", Some(2)).await.unwrap();
    assert_eq!(state.hierarchy_version, 11);
    let write = &sent(&mock)[1];
    assert_eq!(write.0, "POST");
    assert_eq!(
        write.2.as_ref().unwrap(),
        &json!({
            "folderHierarchyVersion": 10,
            "supportsAdditionalSystemGeneratedFolders": true,
            "supportsSliceItems": true,
            "actions": [{"action": "AddItem", "folderId": FOLDER, "itemId": "b", "itemIndex": 2}]
        })
    );
}

#[tokio::test]
async fn pin_without_index_appends() {
    let mock = Mock::new(vec![(200, folders(10, &[])), (200, folders(11, &["b"]))]);
    pins(&mock).pin_chat("b", None).await.unwrap();
    let requests = sent(&mock);
    let action = &requests[1].2.as_ref().unwrap()["actions"][0];
    assert_eq!(action["action"], "AddItem");
    assert!(action.get("itemIndex").is_none());
}

#[tokio::test]
async fn unpin_and_reorder_bodies() {
    let mock = Mock::new(vec![
        (200, folders(10, &["a", "b"])),
        (200, folders(11, &["a"])),
        (200, folders(11, &["a"])),
        (200, folders(12, &["a"])),
    ]);
    let pins = pins(&mock);
    pins.unpin_chat("b").await.unwrap();
    pins.reorder_chats(&["b".to_owned(), "a".to_owned()])
        .await
        .unwrap();
    let requests = sent(&mock);
    assert_eq!(
        requests[1].2.as_ref().unwrap()["actions"][0],
        json!({"action": "RemoveItem", "folderId": FOLDER, "itemId": "b"})
    );
    assert_eq!(
        requests[3].2.as_ref().unwrap()["actions"][0],
        json!({"action": "ReorderItems", "folderId": FOLDER, "newItemsOrder": ["b", "a"]})
    );
    assert_eq!(
        requests[3].2.as_ref().unwrap()["folderHierarchyVersion"],
        11
    );
}

#[tokio::test]
async fn version_mismatch_retries_once_with_the_state_from_the_412_body() {
    let mock = Mock::new(vec![
        (200, folders(10, &["a"])),
        (412, folders(15, &["a", "x"])),
        (200, folders(16, &["a", "x", "b"])),
    ]);
    let state = pins(&mock).pin_chat("b", None).await.unwrap();
    assert_eq!(state.chat_ids, ["a", "x", "b"]);
    let requests = sent(&mock);
    assert_eq!(requests.len(), 3);
    assert_eq!(
        requests[2].2.as_ref().unwrap()["folderHierarchyVersion"],
        15
    );
}

#[tokio::test]
async fn unparseable_412_body_falls_back_to_a_fresh_read() {
    let mock = Mock::new(vec![
        (200, folders(10, &[])),
        (412, json!("stale")),
        (200, folders(20, &[])),
        (200, folders(21, &["b"])),
    ]);
    pins(&mock).pin_chat("b", None).await.unwrap();
    let requests = sent(&mock);
    assert_eq!(requests[2].0, "GET");
    assert_eq!(
        requests[3].2.as_ref().unwrap()["folderHierarchyVersion"],
        20
    );
}

#[tokio::test]
async fn second_version_mismatch_gives_up() {
    let mock = Mock::new(vec![
        (200, folders(10, &[])),
        (412, folders(11, &[])),
        (412, folders(12, &[])),
    ]);
    let error = pins(&mock).pin_chat("b", None).await.unwrap_err();
    assert!(matches!(error, Error::VersionConflict));
    assert_eq!(sent(&mock).len(), 3);
}

#[tokio::test]
async fn already_pinned_and_not_pinned_are_no_ops() {
    for code in ["ConversationAlreadyLinkedWithFolder", "ItemNotFound"] {
        let mock = Mock::new(vec![
            (200, folders(10, &["a"])),
            (
                400,
                json!({"error": {"errorCode": code, "httpCode": 400, "errorMessage": "x"}}),
            ),
            (200, folders(10, &["a"])),
        ]);
        let state = pins(&mock).pin_chat("a", None).await.unwrap();
        assert_eq!(state.chat_ids, ["a"]);
        assert_eq!(sent(&mock).len(), 3, "{code}");
    }
}

#[tokio::test]
async fn other_client_errors_surface_with_status() {
    let mock = Mock::new(vec![
        (200, folders(10, &[])),
        (
            400,
            json!({"error": {"errorCode": "BadThing", "errorMessage": "nope"}}),
        ),
    ]);
    let error = pins(&mock).pin_chat("a", None).await.unwrap_err();
    assert!(
        matches!(
            error,
            Error::Session(session::Error::Api { status: 400, .. })
        ),
        "{error}"
    );
}

#[tokio::test]
async fn missing_favorites_folder_blocks_writes() {
    let mock = Mock::new(vec![(
        200,
        json!({"folderHierarchyVersion": 1, "conversationFolders": []}),
    )]);
    let error = pins(&mock).pin_chat("a", None).await.unwrap_err();
    assert!(matches!(error, Error::NoFavoritesFolder));
    assert_eq!(sent(&mock).len(), 1);
}

fn channel_state(version: &str, ids: &[&str]) -> Value {
    json!({"orderVersion": version, "pinChannelOrder": ids})
}

fn header<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request
        .headers
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

#[tokio::test]
async fn channel_pin_sends_if_match_and_rereads() {
    let mock = Mock::new(vec![
        (200, channel_state("5", &["x"])),
        (200, Value::Null),
        (200, channel_state("6", &["x", "c"])),
    ]);
    let state = pins(&mock).pin_channel("c").await.unwrap();
    assert_eq!(state.channel_ids, ["x", "c"]);
    assert_eq!(state.order_version, "6");
    let requests = mock.requests.lock().unwrap();
    assert_eq!(requests[1].method.as_str(), "POST");
    assert!(requests[1].url.ends_with("/pinnedChannels"));
    assert_eq!(
        requests[1].body.as_ref().unwrap(),
        &json!({"newlyPinnedChannels": ["c"]})
    );
    assert_eq!(header(&requests[1], "if-match"), Some("5"));
}

#[tokio::test]
async fn channel_unpin_uses_delete_and_reorder_uses_post() {
    let mock = Mock::new(vec![
        (200, channel_state("5", &["x", "y"])),
        (200, Value::Null),
        (200, channel_state("6", &["x"])),
        (200, channel_state("6", &["x"])),
        (200, Value::Null),
        (200, channel_state("7", &["x"])),
    ]);
    let pins = pins(&mock);
    pins.unpin_channel("y").await.unwrap();
    pins.reorder_channels(&["x".to_owned()]).await.unwrap();
    let requests = mock.requests.lock().unwrap();
    assert_eq!(requests[1].method.as_str(), "DELETE");
    assert_eq!(
        requests[1].body.as_ref().unwrap(),
        &json!({"channelIdsToUnpin": ["y"]})
    );
    assert_eq!(requests[4].method.as_str(), "POST");
    assert_eq!(
        requests[4].body.as_ref().unwrap(),
        &json!({"pinnedChannelOrder": ["x"]})
    );
    assert_eq!(header(&requests[4], "if-match"), Some("6"));
}

#[tokio::test]
async fn channel_version_mismatch_rereads_and_retries_once() {
    let mock = Mock::new(vec![
        (200, channel_state("5", &[])),
        (412, json!({"error": {"errorMessage": "VersionMismatch"}})),
        (200, channel_state("8", &[])),
        (200, Value::Null),
        (200, channel_state("9", &["c"])),
    ]);
    pins(&mock).pin_channel("c").await.unwrap();
    let requests = mock.requests.lock().unwrap();
    assert_eq!(header(&requests[3], "if-match"), Some("8"));
}

#[tokio::test]
async fn channel_second_mismatch_gives_up() {
    let mismatch = json!("VersionMismatch");
    let mock = Mock::new(vec![
        (200, channel_state("5", &[])),
        (412, mismatch.clone()),
        (200, channel_state("6", &[])),
        (412, mismatch),
    ]);
    assert!(matches!(
        pins(&mock).pin_channel("c").await.unwrap_err(),
        Error::VersionConflict
    ));
}

#[tokio::test]
async fn unpinning_a_channel_that_is_not_pinned_is_a_no_op() {
    let mock = Mock::new(vec![
        (200, channel_state("5", &[])),
        (
            412,
            json!({"error": {"errorMessage": "User's pin list does not include all requested ids"}}),
        ),
        (200, channel_state("5", &[])),
    ]);
    let state = pins(&mock).unpin_channel("c").await.unwrap();
    assert!(state.channel_ids.is_empty());
}

#[tokio::test]
async fn pinning_with_a_non_mismatch_412_is_an_error() {
    let mock = Mock::new(vec![
        (200, channel_state("5", &[])),
        (
            412,
            json!({"error": {"errorMessage": "User's pin list does not include all requested ids"}}),
        ),
    ]);
    assert!(matches!(
        pins(&mock).pin_channel("c").await.unwrap_err(),
        Error::Session(session::Error::Api { status: 412, .. })
    ));
}

fn sections(version: i64, custom: &[(&str, &[&str])], pinned: &[&str]) -> Value {
    let mut folders = vec![json!({
        "id": FOLDER, "name": "Pins", "folderType": "Favorites",
        "conversationFolderItems": pinned.iter().map(|id| json!({"conversationId": id})).collect::<Vec<_>>()
    })];
    for (id, items) in custom {
        folders.push(json!({
            "id": id, "name": id, "folderType": "UserCreated",
            "conversationFolderItems": items.iter().map(|item| json!({"conversationId": item})).collect::<Vec<_>>()
        }));
    }
    json!({"folderHierarchyVersion": version, "conversationFolders": folders})
}

#[tokio::test]
async fn move_to_folder_removes_from_the_current_custom_folder_then_adds_in_one_request() {
    let mock = Mock::new(vec![
        (200, sections(10, &[("t~u~A", &["c"]), ("t~u~B", &[])], &[])),
        (200, sections(11, &[("t~u~A", &[]), ("t~u~B", &["c"])], &[])),
    ]);
    let state = pins(&mock).move_to_folder("c", "t~u~B").await.unwrap();
    assert_eq!(state.folders[2].conversation_ids, ["c"]);
    let body = sent(&mock)[1].2.clone().unwrap();
    assert_eq!(body["folderHierarchyVersion"], 10);
    assert_eq!(
        body["actions"],
        json!([
            {"action": "RemoveItem", "folderId": "t~u~A", "itemId": "c"},
            {"action": "AddItem", "folderId": "t~u~B", "itemId": "c"}
        ])
    );
}

#[tokio::test]
async fn move_to_folder_without_a_current_folder_only_adds() {
    let mock = Mock::new(vec![
        (200, sections(10, &[("t~u~B", &[])], &[])),
        (200, sections(11, &[("t~u~B", &["c"])], &[])),
    ]);
    pins(&mock).move_to_folder("c", "t~u~B").await.unwrap();
    assert_eq!(
        sent(&mock)[1].2.as_ref().unwrap()["actions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn move_to_folder_retries_once_and_replans_from_the_412_state() {
    let mock = Mock::new(vec![
        (200, sections(10, &[("t~u~B", &[])], &[])),
        (412, sections(15, &[("t~u~A", &["c"]), ("t~u~B", &[])], &[])),
        (200, sections(16, &[("t~u~A", &[]), ("t~u~B", &["c"])], &[])),
    ]);
    pins(&mock).move_to_folder("c", "t~u~B").await.unwrap();
    let retry = sent(&mock)[2].2.clone().unwrap();
    assert_eq!(retry["folderHierarchyVersion"], 15);
    assert_eq!(retry["actions"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn move_into_the_folder_it_is_already_in_sends_nothing() {
    let mock = Mock::new(vec![(200, sections(10, &[("t~u~B", &["c"])], &[]))]);
    pins(&mock).move_to_folder("c", "t~u~B").await.unwrap();
    assert_eq!(sent(&mock).len(), 1);
}

#[tokio::test]
async fn unknown_target_folder_is_typed() {
    let mock = Mock::new(vec![(200, sections(10, &[], &[]))]);
    let error = pins(&mock).move_to_folder("c", "nope").await.unwrap_err();
    assert!(matches!(error, Error::UnknownFolder(_)));
}

#[tokio::test]
async fn remove_from_folder_sends_remove_item_and_skips_when_absent() {
    let mock = Mock::new(vec![
        (200, sections(10, &[("t~u~A", &["c"])], &[])),
        (200, sections(11, &[("t~u~A", &[])], &[])),
    ]);
    pins(&mock).remove_from_folder("c", "t~u~A").await.unwrap();
    assert_eq!(
        sent(&mock)[1].2.as_ref().unwrap()["actions"],
        json!([{"action": "RemoveItem", "folderId": "t~u~A", "itemId": "c"}])
    );
    let idle = Mock::new(vec![(200, sections(10, &[("t~u~A", &[])], &[]))]);
    pins(&idle).remove_from_folder("c", "t~u~A").await.unwrap();
    assert_eq!(sent(&idle).len(), 1);
}
