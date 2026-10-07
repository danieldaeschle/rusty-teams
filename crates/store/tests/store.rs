use chrono::{DateTime, TimeZone, Utc};
use rusqlite::Connection;
use store::{
    AvatarRecord, ChannelRecord, ChatPreview, ChatRecord, FolderRecord, MemberRecord,
    MessageRecord, Store, SyncState, TeamRecord,
};

fn at(minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 6, 9, minute, 0).unwrap()
}

fn chat(id: &str, title: &str, last: Option<DateTime<Utc>>) -> ChatRecord {
    ChatRecord {
        id: id.to_owned(),
        kind: "group".to_owned(),
        title: title.to_owned(),
        member_summary: "Ada Example, Bob Sample".to_owned(),
        last_message_at: last,
        last_read_at: Some(at(0)),
        unread: true,
        members: vec![
            MemberRecord {
                user_id: Some("user-ada".to_owned()),
                display_name: "Ada Example".to_owned(),
            },
            MemberRecord {
                user_id: None,
                display_name: "Bob Sample".to_owned(),
            },
        ],
        ..ChatRecord::default()
    }
}

fn message(conversation_id: &str, message_id: &str, minute: u32) -> MessageRecord {
    MessageRecord {
        conversation_id: conversation_id.to_owned(),
        message_id: message_id.to_owned(),
        reply_to_id: None,
        sender_id: Some("user-ada".to_owned()),
        sender_name: Some("Ada Example".to_owned()),
        created_at: at(minute),
        edited_at: None,
        deleted: false,
        body_html: format!("<p>message {message_id}</p>"),
        attachments_json: "[]".to_owned(),
        reactions_json: "[]".to_owned(),
        mentions_json: "[]".to_owned(),
    }
}

fn ids(messages: &[MessageRecord]) -> Vec<&str> {
    messages
        .iter()
        .map(|message| message.message_id.as_str())
        .collect()
}

#[test]
fn chats_round_trip_with_members() {
    let store = Store::open_in_memory().unwrap();
    let original = chat("chat-1", "Planning", Some(at(5)));
    store.upsert_chats(std::slice::from_ref(&original)).unwrap();
    assert_eq!(store.chat("chat-1").unwrap(), Some(original));
    assert_eq!(store.chat("missing").unwrap(), None);
}

#[test]
fn chat_upsert_replaces_fields_and_members() {
    let store = Store::open_in_memory().unwrap();
    store
        .upsert_chats(&[chat("chat-1", "Old", Some(at(1)))])
        .unwrap();
    let mut changed = chat("chat-1", "New", Some(at(9)));
    changed.members.truncate(1);
    changed.unread = false;
    store.upsert_chats(&[changed.clone()]).unwrap();
    assert_eq!(store.recent_chats(10).unwrap(), vec![changed]);
}

#[test]
fn recent_chats_are_newest_first_with_unknown_last() {
    let store = Store::open_in_memory().unwrap();
    store
        .upsert_chats(&[
            chat("old", "Old", Some(at(1))),
            chat("none", "None", None),
            chat("new", "New", Some(at(30))),
            chat("mid", "Mid", Some(at(10))),
        ])
        .unwrap();
    let recent = store.recent_chats(3).unwrap();
    let order: Vec<&str> = recent.iter().map(|chat| chat.id.as_str()).collect();
    assert_eq!(order, ["new", "mid", "old"]);
    assert_eq!(store.recent_chats(10).unwrap().last().unwrap().id, "none");
}

#[test]
fn messages_page_backwards_oldest_first() {
    let store = Store::open_in_memory().unwrap();
    let all: Vec<MessageRecord> = (1..=10)
        .map(|minute| message("chat-1", &format!("m{minute:02}"), minute))
        .collect();
    store.upsert_messages(&all).unwrap();
    store
        .upsert_messages(&[message("chat-2", "other", 5)])
        .unwrap();

    let newest = store.messages("chat-1", None, 4).unwrap();
    assert_eq!(ids(&newest), ["m07", "m08", "m09", "m10"]);

    let older = store
        .messages("chat-1", Some(newest[0].created_at), 4)
        .unwrap();
    assert_eq!(ids(&older), ["m03", "m04", "m05", "m06"]);

    let oldest = store
        .messages("chat-1", Some(older[0].created_at), 4)
        .unwrap();
    assert_eq!(ids(&oldest), ["m01", "m02"]);
    assert!(
        store
            .messages("chat-1", Some(oldest[0].created_at), 4)
            .unwrap()
            .is_empty()
    );
    assert_eq!(store.message_count("chat-1").unwrap(), 10);
}

#[test]
fn equal_timestamps_keep_a_stable_order() {
    let store = Store::open_in_memory().unwrap();
    store
        .upsert_messages(&[
            message("c", "b", 1),
            message("c", "a", 1),
            message("c", "c", 1),
        ])
        .unwrap();
    assert_eq!(
        ids(&store.messages("c", None, 10).unwrap()),
        ["a", "b", "c"]
    );
}

#[test]
fn message_upsert_updates_in_place() {
    let store = Store::open_in_memory().unwrap();
    let mut edited = message("c", "m1", 1);
    store
        .upsert_messages(std::slice::from_ref(&edited))
        .unwrap();
    edited.body_html = "<p>edited</p>".to_owned();
    edited.edited_at = Some(at(2));
    edited.reactions_json = r#"[{"type":"like"}]"#.to_owned();
    store
        .upsert_messages(std::slice::from_ref(&edited))
        .unwrap();

    let stored = store.messages("c", None, 10).unwrap();
    assert_eq!(stored, vec![edited.clone()]);
    let by_id = store
        .messages_by_id("c", &["m1".to_owned(), "nope".to_owned()])
        .unwrap();
    assert_eq!(by_id.len(), 1);
    assert_eq!(by_id["m1"], edited);
}

#[test]
fn deleted_flag_and_json_columns_round_trip() {
    let store = Store::open_in_memory().unwrap();
    let mut record = message("c", "m1", 1);
    record.deleted = true;
    record.reply_to_id = Some("m0".to_owned());
    record.attachments_json = r#"[{"name":"a.pdf"}]"#.to_owned();
    record.mentions_json = r#"[{"id":"u1","name":"Ada Example"}]"#.to_owned();
    store
        .upsert_messages(std::slice::from_ref(&record))
        .unwrap();
    assert_eq!(store.messages("c", None, 1).unwrap(), vec![record]);
}

#[test]
fn sync_state_round_trip() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(store.sync_state("c").unwrap(), None);
    let state = SyncState {
        newest_seen: Some(at(9)),
        oldest_loaded: Some(at(1)),
        has_more: false,
        older_cursor: Some("https://graph.microsoft.com/v1.0/next".to_owned()),
        delta_link: Some("https://graph.microsoft.com/v1.0/delta".to_owned()),
    };
    store.set_sync_state("c", &state).unwrap();
    assert_eq!(store.sync_state("c").unwrap(), Some(state));
    let reset = SyncState {
        has_more: true,
        ..SyncState::default()
    };
    store.set_sync_state("c", &reset).unwrap();
    assert_eq!(store.sync_state("c").unwrap(), Some(reset));
}

#[test]
fn sidebar_groups_channels_under_teams() {
    let store = Store::open_in_memory().unwrap();
    store
        .upsert_chats(&[chat("chat-1", "Planning", Some(at(5)))])
        .unwrap();
    store
        .upsert_teams(&[
            TeamRecord {
                id: "t2".to_owned(),
                name: "beta".to_owned(),
            },
            TeamRecord {
                id: "t1".to_owned(),
                name: "Alpha".to_owned(),
            },
        ])
        .unwrap();
    let channel = |id: &str, team_id: &str, name: &str| ChannelRecord {
        id: id.to_owned(),
        team_id: team_id.to_owned(),
        name: name.to_owned(),
        membership_type: Some("standard".to_owned()),
        last_message_at: None,
        unread: false,
    };
    store
        .upsert_channels(&[
            channel("c2", "t1", "Random"),
            channel("c1", "t1", "General"),
            channel("c3", "t2", "General"),
        ])
        .unwrap();

    let sidebar = store.sidebar().unwrap();
    assert_eq!(sidebar.chats.len(), 1);
    let team_names: Vec<&str> = sidebar
        .teams
        .iter()
        .map(|entry| entry.team.name.as_str())
        .collect();
    assert_eq!(team_names, ["Alpha", "beta"]);
    let alpha_channels: Vec<&str> = sidebar.teams[0]
        .channels
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(alpha_channels, ["General", "Random"]);
    assert_eq!(store.channel("c3").unwrap().unwrap().team_id, "t2");
}

#[test]
fn channel_upsert_keeps_known_last_message_time() {
    let store = Store::open_in_memory().unwrap();
    store
        .upsert_teams(&[TeamRecord {
            id: "t1".to_owned(),
            name: "A".to_owned(),
        }])
        .unwrap();
    let mut channel = ChannelRecord {
        id: "c1".to_owned(),
        team_id: "t1".to_owned(),
        name: "General".to_owned(),
        membership_type: None,
        last_message_at: Some(at(7)),
        unread: false,
    };
    store
        .upsert_channels(std::slice::from_ref(&channel))
        .unwrap();
    channel.last_message_at = None;
    store
        .upsert_channels(std::slice::from_ref(&channel))
        .unwrap();
    assert_eq!(
        store.channel("c1").unwrap().unwrap().last_message_at,
        Some(at(7))
    );
}

#[test]
fn pruning_removes_departed_teams_and_channels() {
    let store = Store::open_in_memory().unwrap();
    store
        .upsert_teams(&[
            TeamRecord {
                id: "t1".to_owned(),
                name: "A".to_owned(),
            },
            TeamRecord {
                id: "t2".to_owned(),
                name: "B".to_owned(),
            },
        ])
        .unwrap();
    let channel = |id: &str| ChannelRecord {
        id: id.to_owned(),
        team_id: "t1".to_owned(),
        name: id.to_owned(),
        membership_type: None,
        last_message_at: None,
        unread: false,
    };
    store
        .upsert_channels(&[channel("c1"), channel("c2")])
        .unwrap();
    assert_eq!(
        store
            .remove_channels_except("t1", &["c1".to_owned()])
            .unwrap(),
        1
    );
    assert_eq!(store.remove_teams_except(&["t1".to_owned()]).unwrap(), 1);
    let sidebar = store.sidebar().unwrap();
    assert_eq!(sidebar.teams.len(), 1);
    assert_eq!(sidebar.teams[0].channels.len(), 1);
}

#[test]
fn file_database_uses_wal_persists_and_migrates_once() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("nested").join("cache.sqlite3");
    {
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), 5);
        store.upsert_messages(&[message("c", "m1", 1)]).unwrap();
    }
    let reopened = Store::open(&path).unwrap();
    assert_eq!(reopened.schema_version().unwrap(), 5);
    assert_eq!(reopened.message_count("c").unwrap(), 1);
    drop(reopened);
    let mode: String = rusqlite_open(&path)
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
}

#[test]
fn newer_schema_is_refused() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cache.sqlite3");
    drop(Store::open(&path).unwrap());
    let connection = rusqlite_open(&path);
    connection
        .execute_batch("PRAGMA user_version = 99")
        .unwrap();
    drop(connection);
    let error = Store::open(&path).err().expect("must refuse");
    assert!(error.to_string().contains("99"), "{error}");
}

#[test]
fn old_schema_is_upgraded_in_place() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cache.sqlite3");
    drop(Store::open(&path).unwrap());
    let connection = rusqlite_open(&path);
    connection.execute_batch("DROP TABLE messages; DROP TABLE sync_state; DROP TABLE chats; DROP TABLE chat_members; DROP TABLE channels; DROP TABLE teams; DROP TABLE meta; DROP TABLE avatars; DROP TABLE folder_items; DROP TABLE folders; DROP TABLE pinned_channels; DROP TABLE images; DROP TABLE search_keys; DROP TABLE message_search; DROP TABLE title_search; DROP TABLE team_layout; DROP TABLE channel_layout; PRAGMA user_version = 0").unwrap();
    drop(connection);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.schema_version().unwrap(), 5);
    store.upsert_messages(&[message("c", "m1", 1)]).unwrap();
}

fn rusqlite_open(path: &std::path::Path) -> Connection {
    Connection::open(path).unwrap()
}

#[test]
fn meta_values_and_times_round_trip() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(store.meta("k").unwrap(), None);
    store.set_meta("k", "one").unwrap();
    store.set_meta("k", "two").unwrap();
    assert_eq!(store.meta("k").unwrap().as_deref(), Some("two"));
    assert_eq!(store.meta_time("t").unwrap(), None);
    store.set_meta_time("t", at(7)).unwrap();
    assert_eq!(store.meta_time("t").unwrap(), Some(at(7)));
    store.set_meta("bad", "not a number").unwrap();
    assert_eq!(store.meta_time("bad").unwrap(), None);
}

#[test]
fn chats_can_be_pruned_and_marked_read() {
    let store = Store::open_in_memory().unwrap();
    store
        .upsert_chats(&[
            chat("a", "A", Some(at(1))),
            chat("b", "B", Some(at(2))),
            chat("c", "C", None),
        ])
        .unwrap();
    let times = store
        .chat_last_message_times(&["a".to_owned(), "c".to_owned(), "zzz".to_owned()])
        .unwrap();
    assert_eq!(times.len(), 2);
    assert_eq!(times["a"], Some(at(1)));
    assert_eq!(times["c"], None);

    assert_eq!(store.remove_chats(&["a".to_owned()]).unwrap(), 1);
    assert_eq!(store.remove_chats_except(&["b".to_owned()]).unwrap(), 1);
    assert_eq!(store.chat_count().unwrap(), 1);

    store.mark_chat_read("b", at(30)).unwrap();
    let read = store.chat("b").unwrap().unwrap();
    assert!(!read.unread);
    assert_eq!(read.last_read_at, Some(at(30)));
}

#[test]
fn migration_to_v2_keeps_existing_sync_state() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("old.sqlite3");
    {
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(include_str!("../src/migrations/0001_initial.sql"))
            .unwrap();
        connection.pragma_update(None, "user_version", 1).unwrap();
        connection
            .execute("INSERT INTO sync_state (conversation_id, newest_seen, has_more) VALUES ('c', 5, 1)", [])
            .unwrap();
    }
    let store = Store::open(&path).unwrap();
    let state = store.sync_state("c").unwrap().unwrap();
    assert_eq!(state.delta_link, None);
    assert_eq!(store.schema_version().unwrap(), 5);
}

#[test]
fn preview_updates_only_for_newer_messages() {
    let store = Store::open_in_memory().unwrap();
    store.upsert_chats(&[chat("a", "A", Some(at(5)))]).unwrap();
    let preview = |text| ChatPreview {
        text: Some(text),
        sender_id: Some("user-ada"),
        sender_name: Some("Ada Example"),
        deleted: false,
    };
    assert!(
        !store
            .update_chat_preview("a", at(4), &preview("older"))
            .unwrap()
    );
    assert!(
        store
            .update_chat_preview("a", at(6), &preview("newer"))
            .unwrap()
    );
    let record = store.chat("a").unwrap().unwrap();
    assert_eq!(record.last_message_preview.as_deref(), Some("newer"));
    assert_eq!(
        record.last_message_sender_name.as_deref(),
        Some("Ada Example")
    );
    assert_eq!(record.last_message_at, Some(at(6)));
    assert!(!record.last_message_deleted);
}

#[test]
fn avatars_round_trip_and_report_stale_ids() {
    let store = Store::open_in_memory().unwrap();
    store
        .upsert_avatar(&AvatarRecord {
            user_id: "fresh".into(),
            bytes: Some(vec![1, 2, 3]),
            content_type: "image/jpeg".into(),
            fetched_at: at(50),
        })
        .unwrap();
    store
        .upsert_avatar(&AvatarRecord {
            user_id: "missing".into(),
            bytes: None,
            content_type: String::new(),
            fetched_at: at(1),
        })
        .unwrap();
    assert_eq!(
        store.avatar("fresh").unwrap().unwrap().bytes,
        Some(vec![1, 2, 3])
    );
    assert_eq!(store.avatar("missing").unwrap().unwrap().bytes, None);
    assert!(store.avatar("unknown").unwrap().is_none());
    let ids: Vec<String> = ["fresh", "missing", "unknown"].map(String::from).to_vec();
    assert_eq!(
        store.avatar_ids_needing_fetch(&ids, at(10)).unwrap(),
        ["missing", "unknown"]
    );
}

#[test]
fn folders_and_pinned_channels_are_replaced_in_order() {
    let store = Store::open_in_memory().unwrap();
    let folder = |id: &str, kind: &str, items: &[&str]| FolderRecord {
        id: id.into(),
        name: id.into(),
        kind: kind.into(),
        conversation_ids: items.iter().map(|item| item.to_string()).collect(),
    };
    let first = [
        folder("f1", "Favorites", &["b", "a"]),
        folder("f2", "UserCreated", &[]),
    ];
    store
        .replace_folders(&first, &["c2".into(), "c1".into()])
        .unwrap();
    assert_eq!(store.folders().unwrap(), first);
    assert_eq!(store.pinned_channel_ids().unwrap(), ["c2", "c1"]);
    store
        .replace_folders(&[folder("f3", "UserCreated", &["z"])], &[])
        .unwrap();
    assert_eq!(store.folders().unwrap().len(), 1);
    assert!(store.pinned_channel_ids().unwrap().is_empty());
}

#[test]
fn counts_messages_after_a_time_from_others() {
    let store = Store::open_in_memory().unwrap();
    let mut mine = message("c", "m3", 3);
    mine.sender_id = Some("me".into());
    store
        .upsert_messages(&[message("c", "m1", 1), message("c", "m2", 2), mine])
        .unwrap();
    assert_eq!(
        store.count_messages_after("c", Some(at(1)), "me").unwrap(),
        1
    );
    assert_eq!(store.count_messages_after("c", None, "me").unwrap(), 2);
}

fn html_message(
    conversation_id: &str,
    message_id: &str,
    minute: u32,
    sender: &str,
    html: &str,
) -> MessageRecord {
    MessageRecord {
        sender_name: Some(sender.to_owned()),
        body_html: html.to_owned(),
        ..message(conversation_id, message_id, minute)
    }
}

#[test]
fn fts5_is_available_and_matches_prefixes_with_highlights() {
    let store = Store::open_in_memory().unwrap();
    store
        .upsert_messages(&[
            html_message(
                "c1",
                "m1",
                1,
                "Ada Example",
                "<p>The <b>budget</b> planning is done</p>",
            ),
            html_message("c1", "m2", 2, "Bob Sample", "<p>Lunch at noon</p>"),
            html_message("c2", "m3", 3, "Bob Sample", "<p>Budget approved</p>"),
        ])
        .unwrap();
    let hits = store.search_messages("budg plan", None, 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].message_id, "m1");
    assert_eq!(hits[0].sender_name.as_deref(), Some("Ada Example"));
    assert_eq!(hits[0].created_at, at(1));
    assert!(
        hits[0].snippet.contains("\u{1}budget\u{2}"),
        "{}",
        hits[0].snippet
    );
    let all = store.search_messages("budget", None, 10).unwrap();
    assert_eq!(ids_of(&all), ["m3", "m1"]);
    let scoped = store.search_messages("budget", Some("c1"), 10).unwrap();
    assert_eq!(ids_of(&scoped), ["m1"]);
    assert_eq!(store.search_messages("budget", None, 1).unwrap().len(), 1);
}

fn ids_of(hits: &[store::SearchHit]) -> Vec<&str> {
    hits.iter().map(|hit| hit.message_id.as_str()).collect()
}

#[test]
fn search_filters_by_sender_and_ignores_unsearchable_input() {
    let store = Store::open_in_memory().unwrap();
    store
        .upsert_messages(&[
            html_message("c1", "m1", 1, "Ada Example", "<p>report</p>"),
            html_message("c1", "m2", 2, "Bob Sample", "<p>report</p>"),
        ])
        .unwrap();
    assert_eq!(
        ids_of(&store.search_messages("from:bob report", None, 10).unwrap()),
        ["m2"]
    );
    assert!(store.search_messages("   ", None, 10).unwrap().is_empty());
    assert!(store.search_messages("\"-*", None, 10).unwrap().is_empty());
    assert!(store.search_messages("rep) OR (", None, 10).is_ok());
}

#[test]
fn search_follows_edits_and_deletes() {
    let store = Store::open_in_memory().unwrap();
    store
        .upsert_messages(&[html_message("c1", "m1", 1, "Ada", "<p>alpha</p>")])
        .unwrap();
    store
        .upsert_messages(&[html_message("c1", "m1", 1, "Ada", "<p>beta</p>")])
        .unwrap();
    assert!(store.search_messages("alpha", None, 10).unwrap().is_empty());
    assert_eq!(store.search_messages("beta", None, 10).unwrap().len(), 1);
    let mut deleted = html_message("c1", "m1", 1, "Ada", "");
    deleted.deleted = true;
    store.upsert_messages(&[deleted]).unwrap();
    assert!(store.search_messages("beta", None, 10).unwrap().is_empty());
}

#[test]
fn title_search_follows_chat_renames() {
    let store = Store::open_in_memory().unwrap();
    store
        .upsert_chats(&[chat("c1", "Budget planning", None)])
        .unwrap();
    assert_eq!(
        store.search_conversations("budg", 10).unwrap()[0].conversation_id,
        "c1"
    );
    store.upsert_chats(&[chat("c1", "Holiday", None)]).unwrap();
    assert!(store.search_conversations("budget", 10).unwrap().is_empty());
    assert_eq!(store.search_conversations("holi", 10).unwrap().len(), 1);
    assert!(
        store
            .search_conversations("from:holi", 10)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn migration_indexes_rows_that_predate_search() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cache.sqlite3");
    {
        let store = Store::open(&path).unwrap();
        store.upsert_chats(&[chat("c1", "Old chat", None)]).unwrap();
        store
            .upsert_messages(&[html_message("c1", "m1", 1, "Ada", "<p>legacy text</p>")])
            .unwrap();
    }
    {
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "DROP TRIGGER chats_title_insert; DROP TRIGGER chats_title_update; DROP TRIGGER chats_title_delete;
                 DROP TRIGGER channels_title_insert; DROP TRIGGER channels_title_update; DROP TRIGGER channels_title_delete;
                 DROP TABLE images; DROP TABLE search_keys; DROP TABLE message_search; DROP TABLE title_search;
                 DROP TABLE team_layout; DROP TABLE channel_layout;
                 PRAGMA user_version = 3;",
            )
            .unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(store.search_messages("legacy", None, 10).unwrap().len(), 1);
    assert_eq!(store.search_conversations("old", 10).unwrap().len(), 1);
}

fn image(key: &str, minute: u32, size: usize) -> store::ImageRecord {
    store::ImageRecord {
        key: key.to_owned(),
        bytes: vec![7; size],
        content_type: "image/png".to_owned(),
        fetched_at: at(minute),
    }
}

#[test]
fn images_round_trip_and_evict_the_oldest_over_the_cap() {
    let store = Store::open_in_memory().unwrap();
    store.put_image(&image("a", 1, 40), 100).unwrap();
    store.put_image(&image("b", 2, 40), 100).unwrap();
    assert_eq!(store.image("a").unwrap().unwrap().bytes.len(), 40);
    store.put_image(&image("c", 3, 40), 100).unwrap();
    assert!(store.image("a").unwrap().is_none());
    assert!(store.has_image("b").unwrap());
    assert!(store.has_image("c").unwrap());
    assert_eq!(store.image_cache_bytes().unwrap(), 80);
    store.put_image(&image("big", 0, 500), 100).unwrap();
    assert!(store.has_image("big").unwrap());
    assert_eq!(store.image_cache_bytes().unwrap(), 500);
}

#[test]
fn first_message_after_from_others_skips_own_and_deleted() {
    let store = Store::open_in_memory().unwrap();
    let mut own = message("c1", "own", 2);
    own.sender_id = Some("me".to_owned());
    let mut gone = message("c1", "gone", 3);
    gone.deleted = true;
    store
        .upsert_messages(&[message("c1", "old", 1), own, gone, message("c1", "new", 4)])
        .unwrap();
    assert_eq!(
        store
            .first_message_after_from_others("c1", Some(at(1)), "me")
            .unwrap()
            .as_deref(),
        Some("new")
    );
    assert_eq!(
        store
            .first_message_after_from_others("c1", None, "me")
            .unwrap()
            .as_deref(),
        Some("old")
    );
    assert!(
        store
            .first_message_after_from_others("c1", Some(at(4)), "me")
            .unwrap()
            .is_none()
    );
}

#[test]
fn a_team_is_found_by_id() {
    let store = Store::open_in_memory().unwrap();
    store
        .upsert_teams(&[TeamRecord {
            id: "t1".to_owned(),
            name: "Alpha".to_owned(),
        }])
        .unwrap();
    assert_eq!(store.team("t1").unwrap().unwrap().name, "Alpha");
    assert!(store.team("nope").unwrap().is_none());
}

#[test]
fn sync_does_not_resurrect_a_locally_read_chat() {
    let store = Store::open_in_memory().unwrap();
    store
        .upsert_chats(&[chat("chat-1", "Planning", Some(at(5)))])
        .unwrap();
    store.mark_chat_read("chat-1", at(10)).unwrap();

    store
        .upsert_chats(&[chat("chat-1", "Planning", Some(at(5)))])
        .unwrap();
    let stale = store.chat("chat-1").unwrap().unwrap();
    assert!(!stale.unread);
    assert_eq!(stale.last_read_at, Some(at(10)));

    store
        .upsert_chats(&[chat("chat-1", "Planning", Some(at(12)))])
        .unwrap();
    assert!(store.chat("chat-1").unwrap().unwrap().unread);
}
