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

#[tokio::test]
async fn collapse_puts_only_the_flag_and_the_lock_on_the_folder_url() {
    let mock = Mock::new(vec![(200, folders(10, &[])), (200, json!({}))]);
    pins(&mock)
        .set_folder_expanded("t~u~MutedChats", false)
        .await
        .unwrap();
    let requests = sent(&mock);
    assert_eq!(requests[1].0, "PUT");
    assert_eq!(
        requests[1].1,
        "https://teams.cloud.microsoft/api/csa/emea/api/v1/teams/users/me/conversationFolders/t~u~MutedChats?supportsAdditionalSystemGeneratedFolders=true&supportsSliceItems=true"
    );
    assert_eq!(
        requests[1].2,
        Some(json!({
            "folderHierarchyVersion": 10,
            "supportsAdditionalSystemGeneratedFolders": true,
            "supportsSliceItems": true,
            "isExpanded": false,
        }))
    );
}

#[tokio::test]
async fn collapse_retries_once_with_the_version_from_the_412_body() {
    let mock = Mock::new(vec![
        (200, folders(10, &[])),
        (412, folders(15, &[])),
        (200, json!({})),
    ]);
    pins(&mock)
        .set_folder_expanded("t~u~MutedChats", true)
        .await
        .unwrap();
    let requests = sent(&mock);
    assert_eq!(requests.len(), 3);
    assert_eq!(
        requests[2].2.as_ref().unwrap()["folderHierarchyVersion"],
        15
    );
    assert_eq!(requests[2].2.as_ref().unwrap()["isExpanded"], true);
}

#[tokio::test]
async fn collapse_gives_up_after_a_second_version_mismatch() {
    let mock = Mock::new(vec![
        (200, folders(10, &[])),
        (412, folders(11, &[])),
        (412, folders(12, &[])),
    ]);
    let outcome = pins(&mock)
        .set_folder_expanded("t~u~MutedChats", true)
        .await;
    assert!(matches!(outcome, Err(Error::VersionConflict)));
}

fn ordered_sections(version: i64, order: &[&str]) -> Value {
    let mut state = sections(version, &[("t~u~A", &["c"]), ("t~u~B", &[])], &[]);
    state["conversationFolderOrder"] = json!(order);
    state
}

const FOLDER_URL: &str =
    "https://teams.cloud.microsoft/api/csa/emea/api/v1/teams/users/me/conversationFolders";
const FOLDER_QUERY: &str = "?supportsAdditionalSystemGeneratedFolders=true&supportsSliceItems=true";

#[tokio::test]
async fn create_folder_posts_add_folder_with_the_name() {
    let mock = Mock::new(vec![
        (200, sections(10, &[], &[])),
        (200, sections(11, &[("new", &[])], &[])),
        (200, sections(11, &[("new", &[])], &[])),
    ]);
    let state = pins(&mock).create_folder("zz").await.unwrap();
    assert_eq!(state.folders.len(), 2);
    assert_eq!(sent(&mock)[2].0, "GET");
    let requests = sent(&mock);
    assert_eq!(requests[1].0, "POST");
    assert_eq!(requests[1].1, format!("{FOLDER_URL}{FOLDER_QUERY}"));
    assert_eq!(
        requests[1].2,
        Some(json!({
            "folderHierarchyVersion": 10,
            "supportsAdditionalSystemGeneratedFolders": true,
            "supportsSliceItems": true,
            "folderName": "zz",
            "actions": [{"action": "AddFolder"}],
        }))
    );
}

#[tokio::test]
async fn folder_create_rename_delete_ignore_a_trimmed_answer_and_reread() {
    let full = || sections(11, &[("t~u~A", &[]), ("t~u~B", &[])], &[]);
    let trimmed = json!({"folderHierarchyVersion": 11, "conversationFolders": []});
    for operation in ["create", "rename", "delete"] {
        let mock = Mock::new(vec![
            (200, sections(10, &[("t~u~A", &[]), ("t~u~B", &[])], &[])),
            (200, trimmed.clone()),
            (200, full()),
        ]);
        let pins = pins(&mock);
        let state = match operation {
            "create" => pins.create_folder("zz").await,
            "rename" => pins.rename_folder("t~u~A", "x").await,
            _ => pins.delete_folder("t~u~A").await,
        }
        .unwrap();
        assert_eq!(state.folders.len(), 3, "{operation}");
        assert_eq!(sent(&mock).len(), 3, "{operation}");
    }
}

#[tokio::test]
async fn folder_action_posts_still_take_a_full_answer_as_state() {
    let mut partial = sections(11, &[("t~u~B", &["c"])], &[]);
    partial["isPartialResponse"] = json!(true);
    let mock = Mock::new(vec![
        (200, sections(10, &[("t~u~B", &[])], &[])),
        (200, partial),
        (200, sections(11, &[("t~u~B", &["c"])], &[])),
    ]);
    pins(&mock).move_to_folder("c", "t~u~B").await.unwrap();
    assert_eq!(sent(&mock).len(), 3);
    let idle = Mock::new(vec![
        (200, sections(10, &[("t~u~B", &[])], &[])),
        (200, sections(11, &[("t~u~B", &["c"])], &[])),
    ]);
    pins(&idle).move_to_folder("c", "t~u~B").await.unwrap();
    assert_eq!(sent(&idle).len(), 2);
}

#[tokio::test]
async fn rename_folder_puts_the_name_on_the_folder_url() {
    let mock = Mock::new(vec![
        (200, sections(10, &[("t~u~A", &[])], &[])),
        (200, sections(11, &[("t~u~A", &[])], &[])),
        (200, sections(11, &[("t~u~A", &[])], &[])),
    ]);
    pins(&mock).rename_folder("t~u~A", "Todo").await.unwrap();
    let requests = sent(&mock);
    assert_eq!(requests[1].0, "PUT");
    assert_eq!(requests[1].1, format!("{FOLDER_URL}/t~u~A{FOLDER_QUERY}"));
    assert_eq!(requests[1].2.as_ref().unwrap()["folderName"], "Todo");
    assert_eq!(
        requests[1].2.as_ref().unwrap()["folderHierarchyVersion"],
        10
    );
}

#[tokio::test]
async fn rename_or_delete_of_a_system_folder_sends_nothing() {
    let mock = Mock::new(vec![
        (200, sections(10, &[], &[])),
        (200, sections(10, &[], &[])),
    ]);
    let rename = pins(&mock).rename_folder(FOLDER, "x").await.unwrap_err();
    let delete = pins(&mock).delete_folder(FOLDER).await.unwrap_err();
    assert!(matches!(rename, Error::SystemFolder(_)));
    assert!(matches!(delete, Error::SystemFolder(_)));
    assert_eq!(sent(&mock).len(), 2);
}

#[tokio::test]
async fn delete_folder_sends_delete_with_only_the_lock() {
    let mock = Mock::new(vec![
        (200, sections(10, &[("t~u~A", &["c"])], &[])),
        (200, json!({})),
        (200, sections(11, &[], &[])),
    ]);
    let state = pins(&mock).delete_folder("t~u~A").await.unwrap();
    assert_eq!(state.folders.len(), 1);
    let requests = sent(&mock);
    assert_eq!(requests[1].0, "DELETE");
    assert_eq!(requests[1].1, format!("{FOLDER_URL}/t~u~A{FOLDER_QUERY}"));
    assert_eq!(
        requests[1].2,
        Some(json!({
            "folderHierarchyVersion": 10,
            "supportsAdditionalSystemGeneratedFolders": true,
            "supportsSliceItems": true,
        }))
    );
    let gone = Mock::new(vec![(200, sections(10, &[], &[]))]);
    pins(&gone).delete_folder("t~u~A").await.unwrap();
    assert_eq!(sent(&gone).len(), 1);
}

#[tokio::test]
async fn move_folder_sends_the_full_order_with_the_folder_swapped() {
    let order = ["q", "t~u~A", FOLDER, "t~u~B", "r"];
    let swapped = ["q", FOLDER, "t~u~A", "t~u~B", "r"];
    let mock = Mock::new(vec![
        (200, ordered_sections(10, &order)),
        (200, ordered_sections(11, &swapped)),
    ]);
    let state = pins(&mock).move_folder("t~u~A", FOLDER).await.unwrap();
    assert_eq!(state.order, swapped);
    let requests = sent(&mock);
    assert_eq!(requests[1].0, "POST");
    assert_eq!(
        requests[1].2.as_ref().unwrap()["actions"],
        json!([{"action": "ReorderFolders", "newFoldersOrder": swapped}])
    );
}

#[tokio::test]
async fn move_folder_onto_itself_sends_nothing() {
    let mock = Mock::new(vec![(200, ordered_sections(10, &["t~u~A", "t~u~B"]))]);
    pins(&mock).move_folder("t~u~A", "t~u~A").await.unwrap();
    assert_eq!(sent(&mock).len(), 1);
}
