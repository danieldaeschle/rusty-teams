use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, TimeZone, Utc};
use graph::{
    Channel, Chat, DriveFolder, HostedImage, Member, Message, MessageExtras, MessageTarget,
    OutgoingMention, Photo, Presence, SharedFile, Team, UploadDestination, UploadedFile, User,
};
use serde_json::json;
use store::{ChannelLayoutRecord, ChannelRecord, Store, TeamLayoutRecord, TeamRecord};
use teams_core::{
    Availability, BoxFuture, ChatFolder, ChatsPage, CoreEvent, DeltaPage, Error, FolderKind,
    FolderSource, MentionCandidate, MentionInput, MentionTarget, PersonSource, Remote, RemotePage,
    Result, SyncConfig, SyncEngine,
};

const ME: &str = "user-me";
const CHAT: &str = "19:chat@thread.v2";
const CHANNEL: &str = "19:channel@thread.tacv2";

fn base() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 6, 8, 0, 0).unwrap()
}

fn message(id: u32, minute: i64, body: &str) -> Message {
    serde_json::from_value(json!({
        "id": format!("m{id:04}"),
        "messageType": "message",
        "createdDateTime": (base() + Duration::minutes(minute)).to_rfc3339(),
        "from": {"user": {"id": "user-ada", "displayName": "Ada Example"}},
        "body": {"contentType": "html", "content": body},
    }))
    .unwrap()
}

fn numbered(count: u32) -> Vec<Message> {
    (1..=count)
        .map(|index| message(index, i64::from(index), &format!("<p>n{index}</p>")))
        .collect()
}

fn chat(
    id: &str,
    topic: Option<&str>,
    last_minute: i64,
    read_minute: i64,
    from: &str,
    hidden: bool,
) -> Chat {
    serde_json::from_value(json!({
        "id": id,
        "topic": topic,
        "chatType": if topic.is_some() { "group" } else { "oneOnOne" },
        "lastUpdatedDateTime": (base() + Duration::minutes(last_minute)).to_rfc3339(),
        "members": [
            {"userId": ME, "displayName": "Me Myself"},
            {"userId": "user-ada", "displayName": "Ada Example"}
        ],
        "lastMessagePreview": {"id": "p", "createdDateTime": (base() + Duration::minutes(last_minute)).to_rfc3339(), "from": {"user": {"id": from}}},
        "viewpoint": {"lastMessageReadDateTime": (base() + Duration::minutes(read_minute)).to_rfc3339(), "isHidden": hidden},
    }))
    .unwrap()
}

#[derive(Default)]
struct Fake {
    chats: Mutex<Vec<Chat>>,
    chat_messages: Mutex<Vec<Message>>,
    channel_pages: Mutex<Vec<Vec<Message>>>,
    teams: Mutex<Vec<Team>>,
    failing_team: Mutex<Option<String>>,
    sent: Mutex<Vec<String>>,
    chat_calls: Mutex<usize>,
    chat_page_calls: Mutex<usize>,
    team_calls: Mutex<usize>,
    calls: Mutex<Vec<String>>,
    delta_items: Mutex<Vec<Message>>,
    delta_fails: Mutex<bool>,
    photos: Mutex<HashMap<String, Vec<u8>>>,
    photo_requests: Mutex<Vec<Vec<String>>>,
    presence_answers: Mutex<Vec<Presence>>,
    hosted_requests: Mutex<Vec<String>>,
    mentions_sent: Mutex<Vec<Vec<OutgoingMention>>>,
    extras_sent: Mutex<Vec<MessageExtras>>,
    folder_requests: Mutex<usize>,
    directory_users: Mutex<Vec<User>>,
    horizons: Mutex<Vec<chatsvc::MemberHorizon>>,
    horizon_calls: Mutex<usize>,
}

impl Fake {
    fn set_chat_messages(&self, messages: Vec<Message>) {
        *self.chat_messages.lock().unwrap() = messages;
    }
}

fn page_of(items: Vec<Message>, next_link: Option<String>) -> Result<RemotePage> {
    Ok(RemotePage { items, next_link })
}

struct Handle(Arc<Fake>);

impl std::ops::Deref for Handle {
    type Target = Fake;

    fn deref(&self) -> &Fake {
        &self.0
    }
}

fn describe(target: &MessageTarget) -> String {
    match target {
        MessageTarget::Chat { message_id, .. } => message_id.clone(),
        MessageTarget::Channel {
            message_id,
            root_id,
            ..
        } => format!("channel:{}:{message_id}", root_id.as_deref().unwrap_or("-")),
    }
}

fn unsupported<T>() -> Result<T> {
    Err(Error::Unsupported("not faked"))
}

impl Fake {
    fn record(&self, call: String) {
        self.calls.lock().unwrap().push(call);
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn sorted_chats(&self) -> Vec<Chat> {
        let mut chats = self.chats.lock().unwrap().clone();
        chats.sort_by_key(|chat| std::cmp::Reverse(chat.last_message_time()));
        chats
    }

    fn chats_from(&self, offset: usize, top: usize) -> ChatsPage {
        *self.chat_page_calls.lock().unwrap() += 1;
        let chats = self.sorted_chats();
        let items: Vec<Chat> = chats.iter().skip(offset).take(top).cloned().collect();
        let next_link =
            (offset + top < chats.len()).then(|| format!("chats:{}:{top}", offset + top));
        ChatsPage { items, next_link }
    }
}

impl Remote for Handle {
    async fn me(&self) -> Result<User> {
        Ok(serde_json::from_value(json!({"id": ME, "displayName": "Me Myself"})).unwrap())
    }

    async fn chats_page(&self, top: usize) -> Result<ChatsPage> {
        Ok(self.chats_from(0, top))
    }

    async fn chats_at(&self, next_link: &str) -> Result<ChatsPage> {
        let mut parts = next_link.trim_start_matches("chats:").split(':');
        let offset = parts.next().unwrap().parse().unwrap();
        let top = parts.next().unwrap().parse().unwrap();
        Ok(self.chats_from(offset, top))
    }

    async fn chat(&self, _chat_id: &str) -> Result<Chat> {
        unsupported()
    }

    async fn chat_members(&self, _chat_id: &str) -> Result<Vec<Member>> {
        Ok(vec![
            serde_json::from_value(
                json!({"userId": ME, "tenantId": "tenant-1", "displayName": "Me Myself"}),
            )
            .unwrap(),
        ])
    }

    async fn joined_teams(&self) -> Result<Vec<Team>> {
        *self.team_calls.lock().unwrap() += 1;
        Ok(self.teams.lock().unwrap().clone())
    }

    async fn channels_for_teams(&self, team_ids: &[String]) -> Result<Vec<Result<Vec<Channel>>>> {
        let failing = self.failing_team.lock().unwrap().clone();
        Ok(team_ids
            .iter()
            .map(|team_id| {
                if failing.as_deref() == Some(team_id.as_str()) {
                    return Err(Error::Unsupported("forced failure"));
                }
                Ok(vec![serde_json::from_value(json!({"id": CHANNEL, "displayName": "General", "membershipType": "standard"})).unwrap()])
            })
            .collect())
    }

    async fn chat_messages(
        &self,
        _chat_id: &str,
        before: Option<DateTime<Utc>>,
        top: usize,
    ) -> Result<RemotePage> {
        *self.chat_calls.lock().unwrap() += 1;
        let mut matching: Vec<Message> = self
            .chat_messages
            .lock()
            .unwrap()
            .iter()
            .filter(|message| {
                before.is_none_or(|before| message.created_date_time.unwrap() < before)
            })
            .cloned()
            .collect();
        matching.sort_by_key(|message| std::cmp::Reverse(message.created_date_time));
        let has_more = matching.len() > top;
        matching.truncate(top);
        page_of(
            matching,
            has_more.then(|| "https://graph.microsoft.com/v1.0/next".to_owned()),
        )
    }

    async fn chat_message(&self, _chat_id: &str, message_id: &str) -> Result<Message> {
        self.record(format!("chat_message {message_id}"));
        self.chat_messages
            .lock()
            .unwrap()
            .iter()
            .find(|message| message.id == message_id)
            .cloned()
            .ok_or(Error::Unsupported("missing message"))
    }

    async fn channel_messages(
        &self,
        _team_id: &str,
        _channel_id: &str,
        _top: usize,
    ) -> Result<RemotePage> {
        let pages = self.channel_pages.lock().unwrap();
        page_of(
            pages[0].clone(),
            (pages.len() > 1).then(|| "page:1".to_owned()),
        )
    }

    async fn channel_messages_at(&self, next_link: &str) -> Result<RemotePage> {
        let index: usize = next_link.trim_start_matches("page:").parse().unwrap();
        let pages = self.channel_pages.lock().unwrap();
        page_of(
            pages[index].clone(),
            (pages.len() > index + 1).then(|| format!("page:{}", index + 1)),
        )
    }

    async fn channel_message(
        &self,
        _team_id: &str,
        _channel_id: &str,
        message_id: &str,
    ) -> Result<Message> {
        self.record(format!("channel_message {message_id}"));
        self.channel_pages
            .lock()
            .unwrap()
            .iter()
            .flatten()
            .find(|message| message.id == message_id)
            .cloned()
            .ok_or(Error::Unsupported("missing message"))
    }

    async fn channel_reply(
        &self,
        _team_id: &str,
        _channel_id: &str,
        message_id: &str,
        reply_id: &str,
    ) -> Result<Message> {
        self.record(format!("channel_reply {message_id} {reply_id}"));
        self.channel_pages
            .lock()
            .unwrap()
            .iter()
            .flatten()
            .filter(|root| root.id == message_id)
            .flat_map(|root| root.replies.iter())
            .find(|reply| reply.id == reply_id)
            .cloned()
            .ok_or(Error::Unsupported("missing reply"))
    }

    async fn channel_delta(
        &self,
        _team_id: &str,
        _channel_id: &str,
        modified_after: Option<DateTime<Utc>>,
    ) -> Result<DeltaPage> {
        self.record(format!("channel_delta since={}", modified_after.is_some()));
        if *self.delta_fails.lock().unwrap() {
            return unsupported();
        }
        Ok(DeltaPage {
            items: self.delta_items.lock().unwrap().clone(),
            next_link: None,
            delta_link: Some("delta:1".to_owned()),
        })
    }

    async fn channel_delta_at(&self, link: &str) -> Result<DeltaPage> {
        self.record(format!("channel_delta_at {link}"));
        if *self.delta_fails.lock().unwrap() {
            return unsupported();
        }
        Ok(DeltaPage {
            items: std::mem::take(&mut *self.delta_items.lock().unwrap()),
            next_link: None,
            delta_link: Some("delta:2".to_owned()),
        })
    }

    async fn channel_replies(
        &self,
        _team_id: &str,
        _channel_id: &str,
        root_ids: &[String],
    ) -> Result<Vec<Result<Vec<Message>>>> {
        Ok(root_ids.iter().map(|_| Ok(Vec::new())).collect())
    }

    async fn send_chat_message(
        &self,
        _chat_id: &str,
        html: &str,
        mentions: &[OutgoingMention],
        extras: &MessageExtras,
    ) -> Result<Message> {
        self.sent.lock().unwrap().push(html.to_owned());
        self.mentions_sent.lock().unwrap().push(mentions.to_vec());
        self.extras_sent.lock().unwrap().push(extras.clone());
        Ok(message(9000, 500, html))
    }

    async fn send_channel_message(
        &self,
        _team_id: &str,
        _channel_id: &str,
        html: &str,
        subject: Option<&str>,
        mentions: &[OutgoingMention],
        extras: &MessageExtras,
    ) -> Result<Message> {
        self.record(format!("post subject={subject:?}"));
        self.mentions_sent.lock().unwrap().push(mentions.to_vec());
        self.extras_sent.lock().unwrap().push(extras.clone());
        Ok(message(9001, 501, html))
    }

    async fn reply_to_channel_message(
        &self,
        _team_id: &str,
        _channel_id: &str,
        message_id: &str,
        html: &str,
        mentions: &[OutgoingMention],
        extras: &MessageExtras,
    ) -> Result<Message> {
        self.record(format!("reply to {message_id}"));
        self.mentions_sent.lock().unwrap().push(mentions.to_vec());
        self.extras_sent.lock().unwrap().push(extras.clone());
        Ok(message(9002, 502, html))
    }

    async fn mark_chat_read(&self, chat_id: &str, user_id: &str, tenant_id: &str) -> Result<()> {
        self.record(format!("mark_read {chat_id} {user_id} {tenant_id}"));
        Ok(())
    }

    async fn set_reaction(&self, target: &MessageTarget, reaction_type: &str) -> Result<()> {
        self.record(format!("set_reaction {} {reaction_type}", describe(target)));
        Ok(())
    }

    async fn unset_reaction(&self, target: &MessageTarget, reaction_type: &str) -> Result<()> {
        self.record(format!(
            "unset_reaction {} {reaction_type}",
            describe(target)
        ));
        Ok(())
    }

    async fn edit_message(
        &self,
        target: &MessageTarget,
        html: &str,
        mentions: &[OutgoingMention],
        extras: &MessageExtras,
    ) -> Result<()> {
        self.record(format!("edit {} {html}", describe(target)));
        self.mentions_sent.lock().unwrap().push(mentions.to_vec());
        self.extras_sent.lock().unwrap().push(extras.clone());
        Ok(())
    }

    async fn delete_file(&self, file: &UploadedFile) -> Result<()> {
        self.record(format!("delete_file {}", file.item_id));
        Ok(())
    }

    async fn soft_delete_message(&self, user_id: &str, target: &MessageTarget) -> Result<()> {
        self.record(format!("soft_delete {user_id} {}", describe(target)));
        Ok(())
    }

    async fn create_one_on_one(&self, my_user_id: &str, user_id: &str) -> Result<Chat> {
        self.record(format!("one_on_one {my_user_id} {user_id}"));
        Ok(chat("19:new@thread.v2", None, 40, 40, ME, false))
    }

    async fn create_group(
        &self,
        my_user_id: &str,
        user_ids: &[String],
        topic: Option<&str>,
    ) -> Result<Chat> {
        self.record(format!(
            "group {my_user_id} {} {topic:?}",
            user_ids.join(",")
        ));
        Ok(chat("19:group@thread.v2", topic, 41, 41, ME, false))
    }

    async fn reply_with_quote(
        &self,
        chat_id: &str,
        quoted_message_id: &str,
        html: &str,
        mentions: &[OutgoingMention],
        extras: &MessageExtras,
    ) -> Result<Message> {
        self.record(format!("quote_reply {chat_id} {quoted_message_id} {html}"));
        self.mentions_sent.lock().unwrap().push(mentions.to_vec());
        self.extras_sent.lock().unwrap().push(extras.clone());
        Ok(message(9003, 503, html))
    }

    async fn channel_files_folder(&self, team_id: &str, channel_id: &str) -> Result<DriveFolder> {
        *self.folder_requests.lock().unwrap() += 1;
        self.record(format!("files_folder {team_id} {channel_id}"));
        Ok(DriveFolder {
            drive_id: "drive-1".into(),
            item_id: "folder-1".into(),
        })
    }

    async fn upload_file(
        &self,
        destination: &UploadDestination,
        file_name: &str,
        bytes: &[u8],
        progress: &(dyn Fn(u8) + Send + Sync),
    ) -> Result<UploadedFile> {
        let target = match destination {
            UploadDestination::ChatFiles => "chat-files".to_owned(),
            UploadDestination::Folder(folder) => format!("{}/{}", folder.drive_id, folder.item_id),
        };
        self.record(format!("upload {target} {file_name} {}", bytes.len()));
        progress(50);
        progress(100);
        Ok(UploadedFile {
            drive_id: "drive-1".into(),
            item_id: "item-1".into(),
            name: file_name.to_owned(),
            web_url: "https://files.example/web".into(),
            web_dav_url: Some("https://files.example/dav".into()),
            etag: "\"{GUID-1},2\"".into(),
        })
    }

    async fn resolve_share(&self, open_url: &str) -> Result<SharedFile> {
        self.record(format!("resolve {open_url}"));
        Ok(SharedFile {
            name: "big.bin".into(),
            size: 2 * graph::DOWNLOAD_CHUNK_BYTES + 3,
            download_url: if open_url.contains("short") {
                "https://dl.example/short".into()
            } else if open_url.contains("grown") {
                "https://dl.example/grown".into()
            } else {
                "https://dl.example/big".into()
            },
        })
    }

    async fn download_range(
        &self,
        download_url: &str,
        start: u64,
        end: Option<u64>,
    ) -> Result<Vec<u8>> {
        let total = 2 * graph::DOWNLOAD_CHUNK_BYTES + 3;
        let shown_end = end.map_or(String::new(), |end| end.to_string());
        self.record(format!("range {download_url} {start}-{shown_end}"));
        let length = match end {
            _ if download_url.contains("short") => 1,
            Some(end) => end - start + 1,
            None if download_url.contains("grown") => total - start + 10,
            None => total - start,
        };
        Ok(vec![7; length as usize])
    }

    async fn share_file(&self, file: &UploadedFile, user_ids: &[String]) -> Result<()> {
        self.record(format!("share {} {}", file.item_id, user_ids.join(",")));
        Ok(())
    }

    async fn hosted_content(&self, url: &str) -> Result<Photo> {
        self.hosted_requests.lock().unwrap().push(url.to_owned());
        if url.contains("broken") {
            return unsupported();
        }
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend([
            0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 0, 40, 0, 0, 0, 20,
        ]);
        Ok(Photo {
            bytes,
            content_type: String::new(),
        })
    }

    async fn user_photos(&self, user_ids: &[String]) -> Result<Vec<Result<Option<Photo>>>> {
        self.photo_requests.lock().unwrap().push(user_ids.to_vec());
        let photos = self.photos.lock().unwrap();
        Ok(user_ids
            .iter()
            .map(|user_id| {
                Ok(photos.get(user_id).map(|bytes| Photo {
                    bytes: bytes.clone(),
                    content_type: "image/jpeg".to_owned(),
                }))
            })
            .collect())
    }

    async fn consumption_horizons(
        &self,
        _conversation_id: &str,
    ) -> Result<Vec<chatsvc::MemberHorizon>> {
        *self.horizon_calls.lock().unwrap() += 1;
        Ok(self.horizons.lock().unwrap().clone())
    }

    async fn presences(&self, user_ids: &[String]) -> Result<Vec<Presence>> {
        let answers = self.presence_answers.lock().unwrap();
        Ok(answers
            .iter()
            .filter(|presence| user_ids.contains(&presence.user_id))
            .cloned()
            .collect())
    }

    async fn search_people(&self, query: &str) -> Result<Vec<User>> {
        self.record(format!("search {query}"));
        let configured = self.directory_users.lock().unwrap().clone();
        if !configured.is_empty() {
            return Ok(configured);
        }
        Ok(vec![
            serde_json::from_value(json!({"id": "user-ada", "displayName": "Ada Example"}))
                .unwrap(),
        ])
    }
}

fn engine_with(fake: &Arc<Fake>, config: SyncConfig) -> SyncEngine<Handle> {
    SyncEngine::with_config(
        Handle(fake.clone()),
        Arc::new(Store::open_in_memory().unwrap()),
        config,
    )
}

fn small_pages() -> SyncConfig {
    SyncConfig {
        page_size: 10,
        open_limit: 10,
        ..SyncConfig::default()
    }
}

async fn chat_engine(fake: &Arc<Fake>, config: SyncConfig) -> SyncEngine<Handle> {
    *fake.chats.lock().unwrap() = vec![chat(CHAT, Some("Planning"), 5, 5, ME, false)];
    let engine = engine_with(fake, config);
    engine.refresh_sidebar().await.unwrap();
    engine
}

#[tokio::test]
async fn refresh_sidebar_fills_chats_teams_and_channels() {
    let fake = Arc::new(Fake::default());
    *fake.chats.lock().unwrap() = vec![
        chat("19:read@thread.v2", Some("Read"), 10, 10, "user-ada", false),
        chat("19:unread@thread.v2", None, 30, 10, "user-ada", false),
        chat("19:mine@thread.v2", Some("Mine"), 20, 5, ME, false),
        chat(
            "19:hidden@thread.v2",
            Some("Hidden"),
            40,
            1,
            "user-ada",
            true,
        ),
    ];
    *fake.teams.lock().unwrap() =
        vec![serde_json::from_value(json!({"id": "team-1", "displayName": "Squad"})).unwrap()];
    let engine = engine_with(&fake, SyncConfig::default());
    let mut events = engine.subscribe();

    let summary = engine.refresh_sidebar().await.unwrap();
    assert_eq!(
        (
            summary.chats,
            summary.teams,
            summary.channels,
            summary.failed_teams
        ),
        (4, 1, 1, 0)
    );
    assert_eq!(events.try_recv().unwrap(), CoreEvent::SidebarChanged);

    let sidebar = engine.sidebar().unwrap();
    let order: Vec<(&str, bool)> = sidebar
        .chats
        .iter()
        .map(|chat| (chat.title.as_str(), chat.unread))
        .collect();
    assert_eq!(
        order,
        [("Ada Example", true), ("Mine", false), ("Read", false)]
    );
    assert_eq!(sidebar.chats[0].kind, "oneOnOne");
    assert_eq!(sidebar.chats[1].member_summary, "Ada Example");
    assert_eq!(sidebar.teams[0].channels[0].name, "General");
}

#[tokio::test]
async fn failing_channel_listing_is_reported_not_fatal() {
    let fake = Arc::new(Fake::default());
    *fake.teams.lock().unwrap() = vec![
        serde_json::from_value(json!({"id": "team-ok", "displayName": "Fine"})).unwrap(),
        serde_json::from_value(json!({"id": "team-bad", "displayName": "Broken"})).unwrap(),
    ];
    *fake.failing_team.lock().unwrap() = Some("team-bad".to_owned());
    let engine = engine_with(&fake, SyncConfig::default());
    let mut events = engine.subscribe();
    let summary = engine.refresh_sidebar().await.unwrap();
    assert_eq!(
        (summary.teams, summary.channels, summary.failed_teams),
        (2, 1, 1)
    );
    assert!(matches!(
        events.try_recv().unwrap(),
        CoreEvent::Error { .. }
    ));
    assert_eq!(events.try_recv().unwrap(), CoreEvent::SidebarChanged);
}

#[tokio::test]
async fn cold_open_then_delta() {
    let fake = Arc::new(Fake::default());
    fake.set_chat_messages(numbered(25));
    let engine = chat_engine(&fake, small_pages()).await;
    let mut events = engine.subscribe();

    assert!(engine.open_conversation(CHAT).unwrap().is_empty());
    let first = engine.fetch_newer(CHAT).await.unwrap();
    assert_eq!(first.added.len(), 10);
    assert!(first.updated.is_empty());
    assert_eq!(
        events.try_recv().unwrap(),
        CoreEvent::MessagesChanged {
            conversation_id: CHAT.to_owned()
        }
    );

    let cached = engine.open_conversation(CHAT).unwrap();
    assert_eq!(cached.len(), 10);
    assert_eq!(cached.first().unwrap().message_id, "m0016");
    assert_eq!(cached.last().unwrap().message_id, "m0025");
    *fake.chat_calls.lock().unwrap() = 0;

    let mut all = numbered(25);
    all.extend((26..=28).map(|index| message(index, i64::from(index), "<p>fresh</p>")));
    all[24] = message(25, 25, "<p>edited</p>");
    fake.set_chat_messages(all);
    let delta = engine.fetch_newer(CHAT).await.unwrap();
    let added: Vec<&str> = delta
        .added
        .iter()
        .map(|record| record.message_id.as_str())
        .collect();
    assert_eq!(added, ["m0026", "m0027", "m0028"]);
    assert_eq!(delta.updated.len(), 1);
    assert_eq!(delta.updated[0].body_html, "<p>edited</p>");
    assert_eq!(*fake.chat_calls.lock().unwrap(), 1);

    while events.try_recv().is_ok() {}
    let unchanged = engine.fetch_newer(CHAT).await.unwrap();
    assert!(unchanged.is_empty());
    assert!(events.try_recv().is_err());
}

#[tokio::test]
async fn catch_up_walks_several_pages_until_the_known_message() {
    let fake = Arc::new(Fake::default());
    fake.set_chat_messages(numbered(5));
    let engine = chat_engine(&fake, small_pages()).await;
    engine.fetch_newer(CHAT).await.unwrap();

    fake.set_chat_messages(numbered(35));
    *fake.chat_calls.lock().unwrap() = 0;
    let delta = engine.fetch_newer(CHAT).await.unwrap();
    assert_eq!(delta.added.len(), 30);
    assert_eq!(*fake.chat_calls.lock().unwrap(), 4);
    assert_eq!(engine.store().message_count(CHAT).unwrap(), 35);
}

#[tokio::test]
async fn load_older_pages_back_to_the_start() {
    let fake = Arc::new(Fake::default());
    fake.set_chat_messages(numbered(25));
    let engine = chat_engine(&fake, small_pages()).await;

    let older = engine.load_older(CHAT).await.unwrap();
    assert_eq!(older.len(), 10);
    assert_eq!(older[0].message_id, "m0006");

    let older = engine.load_older(CHAT).await.unwrap();
    assert_eq!(older.first().unwrap().message_id, "m0001");
    assert_eq!(older.len(), 5);
    assert!(!engine.store().sync_state(CHAT).unwrap().unwrap().has_more);
    assert_eq!(engine.store().message_count(CHAT).unwrap(), 25);

    *fake.chat_calls.lock().unwrap() = 0;
    assert!(engine.load_older(CHAT).await.unwrap().is_empty());
    assert_eq!(*fake.chat_calls.lock().unwrap(), 0);
}

#[tokio::test]
async fn system_messages_are_not_cached() {
    let fake = Arc::new(Fake::default());
    let system: Message = serde_json::from_value(json!({
        "id": "sys", "messageType": "systemEventMessage",
        "createdDateTime": (base() + Duration::minutes(50)).to_rfc3339(),
        "body": {"contentType": "html", "content": "<systemEventMessage/>"}
    }))
    .unwrap();
    fake.set_chat_messages(vec![message(1, 1, "<p>a</p>"), system]);
    let engine = chat_engine(&fake, small_pages()).await;
    let delta = engine.fetch_newer(CHAT).await.unwrap();
    assert_eq!(delta.added.len(), 1);
}

#[tokio::test]
async fn deleted_and_text_messages_are_normalised() {
    let fake = Arc::new(Fake::default());
    let deleted: Message = serde_json::from_value(json!({
        "id": "gone", "messageType": "message",
        "createdDateTime": (base() + Duration::minutes(1)).to_rfc3339(),
        "deletedDateTime": (base() + Duration::minutes(2)).to_rfc3339(),
        "body": {"contentType": "html", "content": "<p>secret</p>"}
    }))
    .unwrap();
    let text: Message = serde_json::from_value(json!({
        "id": "plain", "messageType": "message",
        "createdDateTime": (base() + Duration::minutes(3)).to_rfc3339(),
        "from": {"application": {"id": "bot", "displayName": "Build Bot"}},
        "body": {"contentType": "text", "content": " a < b\nnext "}
    }))
    .unwrap();
    fake.set_chat_messages(vec![deleted, text]);
    let engine = chat_engine(&fake, small_pages()).await;
    engine.fetch_newer(CHAT).await.unwrap();
    let cached = engine.open_conversation(CHAT).unwrap();
    assert!(cached[0].deleted);
    assert_eq!(cached[0].body_html, "");
    assert_eq!(cached[1].body_html, "a &lt; b<br>next");
    assert_eq!(cached[1].sender_name.as_deref(), Some("Build Bot"));
    assert_eq!(cached[1].sender_id, None);
}

#[tokio::test]
async fn send_message_converts_markdown_and_caches_the_answer() {
    let fake = Arc::new(Fake::default());
    fake.set_chat_messages(numbered(3));
    let engine = chat_engine(&fake, small_pages()).await;
    engine.fetch_newer(CHAT).await.unwrap();
    let mut events = engine.subscribe();

    let record = engine
        .send_message(CHAT, "hi **there** <x>", None)
        .await
        .unwrap();
    assert_eq!(
        fake.sent.lock().unwrap().as_slice(),
        ["hi <b>there</b> &lt;x&gt;"]
    );
    assert_eq!(record.message_id, "m9000");
    assert_eq!(engine.store().message_count(CHAT).unwrap(), 4);
    assert_eq!(
        events.try_recv().unwrap(),
        CoreEvent::MessagesChanged {
            conversation_id: CHAT.to_owned()
        }
    );
    assert!(
        engine
            .store()
            .sync_state(CHAT)
            .unwrap()
            .unwrap()
            .newest_seen
            .unwrap()
            >= record.created_at
    );
}

#[tokio::test]
async fn unknown_conversation_is_an_error() {
    let fake = Arc::new(Fake::default());
    let engine = engine_with(&fake, SyncConfig::default());
    assert!(matches!(
        engine.fetch_newer("nope").await,
        Err(Error::UnknownConversation(_))
    ));
}

fn thread(id: u32, minute: i64, reply_count: u32) -> Message {
    let mut value = json!({
        "id": format!("m{id:04}"),
        "messageType": "message",
        "createdDateTime": (base() + Duration::minutes(minute)).to_rfc3339(),
        "body": {"contentType": "html", "content": "<p>root</p>"},
        "replies": (0..reply_count).map(|reply| json!({
            "id": format!("r{id:04}-{reply}"),
            "messageType": "message",
            "replyToId": format!("m{id:04}"),
            "createdDateTime": (base() + Duration::minutes(minute + 1 + i64::from(reply))).to_rfc3339(),
            "body": {"contentType": "html", "content": "<p>reply</p>"}
        })).collect::<Vec<_>>()
    });
    value["from"] = json!({"user": {"id": "user-ada", "displayName": "Ada Example"}});
    serde_json::from_value(value).unwrap()
}

#[tokio::test]
async fn channel_threads_are_flattened_and_paged_by_cursor() {
    let fake = Arc::new(Fake::default());
    *fake.teams.lock().unwrap() =
        vec![serde_json::from_value(json!({"id": "team-1", "displayName": "Squad"})).unwrap()];
    *fake.channel_pages.lock().unwrap() = vec![
        vec![thread(30, 30, 2), thread(20, 20, 0)],
        vec![thread(10, 10, 1)],
    ];
    let engine = engine_with(&fake, small_pages());
    engine.refresh_sidebar().await.unwrap();

    let delta = engine.fetch_newer(CHANNEL).await.unwrap();
    assert_eq!(delta.added.len(), 4);
    let reply = delta
        .added
        .iter()
        .find(|record| record.message_id == "r0030-1")
        .unwrap();
    assert_eq!(reply.reply_to_id.as_deref(), Some("m0030"));
    let state = engine.store().sync_state(CHANNEL).unwrap().unwrap();
    assert_eq!(state.older_cursor.as_deref(), Some("page:1"));
    assert!(state.has_more);

    let older = engine.load_older(CHANNEL).await.unwrap();
    assert_eq!(older.len(), 2);
    assert!(
        !engine
            .store()
            .sync_state(CHANNEL)
            .unwrap()
            .unwrap()
            .has_more
    );
    assert!(engine.load_older(CHANNEL).await.unwrap().is_empty());
    assert_eq!(engine.store().message_count(CHANNEL).unwrap(), 6);

    assert!(matches!(
        engine.send_message(CHANNEL, "x", None).await,
        Err(Error::Unsupported(_))
    ));
}

fn many_chats(count: i64) -> Vec<Chat> {
    (1..=count)
        .map(|index| {
            chat(
                &format!("19:c{index:03}@thread.v2"),
                Some("Topic"),
                index,
                index,
                "user-ada",
                false,
            )
        })
        .collect()
}

#[tokio::test]
async fn warm_refresh_stops_after_the_first_unchanged_page() {
    let fake = Arc::new(Fake::default());
    *fake.chats.lock().unwrap() = many_chats(120);
    let engine = engine_with(&fake, SyncConfig::default());
    let cold = engine.refresh_sidebar().await.unwrap();
    assert!(cold.full_chat_refresh);
    assert_eq!(cold.chat_pages, 5);
    assert_eq!(engine.store().chat_count().unwrap(), 120);

    *fake.chat_page_calls.lock().unwrap() = 0;
    *fake.team_calls.lock().unwrap() = 0;
    let warm = engine.refresh_sidebar().await.unwrap();
    assert!(!warm.full_chat_refresh);
    assert_eq!(warm.chat_pages, 1);
    assert_eq!(*fake.chat_page_calls.lock().unwrap(), 1);
    assert_eq!(*fake.team_calls.lock().unwrap(), 0);
    assert!(!warm.teams_refreshed);
}

#[tokio::test]
async fn warm_refresh_walks_on_while_pages_hold_new_activity() {
    let fake = Arc::new(Fake::default());
    *fake.chats.lock().unwrap() = many_chats(120);
    let engine = engine_with(&fake, SyncConfig::default());
    engine.refresh_sidebar().await.unwrap();

    let mut chats = many_chats(120);
    for chat in chats.iter_mut().take(30) {
        *chat = self::chat(&chat.id.clone(), Some("Topic"), 500, 1, "user-ada", false);
    }
    *fake.chats.lock().unwrap() = chats;
    *fake.chat_page_calls.lock().unwrap() = 0;
    let warm = engine.refresh_sidebar().await.unwrap();
    assert_eq!(warm.chat_pages, 3);
}

#[tokio::test]
async fn full_refresh_prunes_chats_the_user_left() {
    let fake = Arc::new(Fake::default());
    *fake.chats.lock().unwrap() = many_chats(5);
    let engine = engine_with(&fake, SyncConfig::default());
    engine.refresh_sidebar().await.unwrap();
    assert_eq!(engine.store().chat_count().unwrap(), 5);

    *fake.chats.lock().unwrap() = many_chats(3);
    engine.refresh_sidebar_full().await.unwrap();
    assert_eq!(engine.store().chat_count().unwrap(), 3);
}

#[tokio::test]
async fn teams_refresh_is_skipped_inside_the_interval() {
    let fake = Arc::new(Fake::default());
    *fake.teams.lock().unwrap() =
        vec![serde_json::from_value(json!({"id": "team-1", "displayName": "Squad"})).unwrap()];
    let engine = engine_with(&fake, SyncConfig::default());
    assert!(engine.refresh_sidebar().await.unwrap().teams_refreshed);
    assert!(!engine.refresh_sidebar().await.unwrap().teams_refreshed);
    assert!(engine.refresh_sidebar_full().await.unwrap().teams_refreshed);
    assert_eq!(*fake.team_calls.lock().unwrap(), 2);
}

async fn channel_engine(fake: &Arc<Fake>) -> SyncEngine<Handle> {
    *fake.teams.lock().unwrap() =
        vec![serde_json::from_value(json!({"id": "team-1", "displayName": "Squad"})).unwrap()];
    *fake.channel_pages.lock().unwrap() = vec![vec![thread(30, 30, 1)]];
    let engine = engine_with(fake, small_pages());
    engine.refresh_sidebar().await.unwrap();
    engine
}

#[tokio::test]
async fn channel_delta_picks_up_new_and_edited_posts_and_stores_the_link() {
    let fake = Arc::new(Fake::default());
    let engine = channel_engine(&fake).await;
    engine.fetch_newer(CHANNEL).await.unwrap();
    let state = engine.store().sync_state(CHANNEL).unwrap().unwrap();
    assert_eq!(state.delta_link.as_deref(), Some("delta:1"));

    let mut edited = thread(30, 30, 0);
    edited.body =
        serde_json::from_value(json!({"contentType": "html", "content": "<p>edited root</p>"}))
            .unwrap();
    *fake.delta_items.lock().unwrap() = vec![edited, thread(40, 40, 0)];
    let delta = engine.fetch_newer(CHANNEL).await.unwrap();
    assert_eq!(
        delta
            .added
            .iter()
            .map(|record| record.message_id.as_str())
            .collect::<Vec<_>>(),
        ["m0040"]
    );
    assert_eq!(delta.updated.len(), 1);
    assert_eq!(delta.updated[0].body_html, "<p>edited root</p>");
    assert!(
        fake.calls()
            .iter()
            .any(|call| call == "channel_delta_at delta:1")
    );
    assert_eq!(
        engine
            .store()
            .sync_state(CHANNEL)
            .unwrap()
            .unwrap()
            .delta_link
            .as_deref(),
        Some("delta:2")
    );
}

#[tokio::test]
async fn channel_delta_failure_falls_back_to_the_first_page() {
    let fake = Arc::new(Fake::default());
    let engine = channel_engine(&fake).await;
    engine.fetch_newer(CHANNEL).await.unwrap();

    *fake.delta_fails.lock().unwrap() = true;
    *fake.channel_pages.lock().unwrap() = vec![vec![thread(50, 50, 0), thread(30, 30, 1)]];
    let delta = engine.fetch_newer(CHANNEL).await.unwrap();
    assert_eq!(
        delta
            .added
            .iter()
            .map(|record| record.message_id.as_str())
            .collect::<Vec<_>>(),
        ["m0050"]
    );
    assert_eq!(
        engine
            .store()
            .sync_state(CHANNEL)
            .unwrap()
            .unwrap()
            .delta_link,
        None
    );
}

#[tokio::test]
async fn channel_sends_post_and_reply_through_graph() {
    let fake = Arc::new(Fake::default());
    let engine = channel_engine(&fake).await;
    engine.fetch_newer(CHANNEL).await.unwrap();

    engine
        .post_to_channel(CHANNEL, "hello", Some("Topic"))
        .await
        .unwrap();
    let reply = engine
        .send_message(CHANNEL, "answer", Some("m0030"))
        .await
        .unwrap();
    assert_eq!(reply.message_id, "m9002");
    let sends: Vec<String> = fake
        .calls()
        .into_iter()
        .filter(|call| !call.starts_with("channel_delta"))
        .collect();
    assert_eq!(sends, ["post subject=Some(\"Topic\")", "reply to m0030"]);
    assert!(engine.post_to_channel(CHAT, "x", None).await.is_err());
}

#[tokio::test]
async fn chat_upload_goes_to_chat_files_and_is_shared_with_the_other_members() {
    let fake = Arc::new(Fake::default());
    let engine = chat_engine(&fake, small_pages()).await;
    let steps = Arc::new(Mutex::new(Vec::new()));
    let seen = steps.clone();

    let reference = engine
        .upload_attachment(CHAT, "plan.pdf", &[1, 2, 3], move |percent| {
            seen.lock().unwrap().push(percent)
        })
        .await
        .unwrap();

    assert_eq!(
        fake.calls(),
        ["upload chat-files plan.pdf 3", "share item-1 user-ada"]
    );
    assert_eq!(*steps.lock().unwrap(), [50, 100]);
    assert_eq!(reference.attachment_id, "GUID-1");
    assert_eq!(reference.content_url, "https://files.example/dav");
    assert_eq!(reference.name, "plan.pdf");
}

#[tokio::test]
async fn channel_upload_uses_the_cached_files_folder_and_shares_nothing() {
    let fake = Arc::new(Fake::default());
    let engine = channel_engine(&fake).await;

    engine
        .upload_attachment(CHANNEL, "a.pdf", &[1], |_| {})
        .await
        .unwrap();
    engine
        .upload_attachment(CHANNEL, "b.pdf", &[1, 2], |_| {})
        .await
        .unwrap();

    assert_eq!(*fake.folder_requests.lock().unwrap(), 1);
    let uploads: Vec<String> = fake
        .calls()
        .into_iter()
        .filter(|call| call.starts_with("upload") || call.starts_with("share"))
        .collect();
    assert_eq!(
        uploads,
        [
            "upload drive-1/folder-1 a.pdf 1",
            "upload drive-1/folder-1 b.pdf 2"
        ]
    );
}

fn uploaded() -> UploadedFile {
    UploadedFile {
        drive_id: "drive-1".into(),
        item_id: "item-1".into(),
        name: "a.pdf".into(),
        web_url: "https://files.example/web".into(),
        web_dav_url: None,
        etag: "\"{GUID-1},2\"".into(),
    }
}

#[tokio::test]
async fn sharing_fails_loudly_when_the_chat_members_are_unknown() {
    let fake = Arc::new(Fake::default());
    let engine = chat_engine(&fake, small_pages()).await;
    engine
        .store()
        .upsert_chats(&[store::ChatRecord {
            id: "19:bare@thread.v2".into(),
            ..Default::default()
        }])
        .unwrap();

    let error = engine
        .share_attachment("19:bare@thread.v2", &uploaded())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("Chat members not loaded yet."));
    engine.share_attachment(CHAT, &uploaded()).await.unwrap();
    assert!(fake.calls().contains(&"share item-1 user-ada".to_owned()));
}

#[tokio::test]
async fn discarding_an_upload_deletes_the_drive_item() {
    let fake = Arc::new(Fake::default());
    let engine = chat_engine(&fake, small_pages()).await;
    engine.discard_attachment(&uploaded()).await.unwrap();
    assert_eq!(fake.calls(), ["delete_file item-1"]);
}

#[tokio::test]
async fn an_edit_keeps_the_images_and_files_of_the_original() {
    let fake = Arc::new(Fake::default());
    let engine = chat_engine(&fake, small_pages()).await;
    engine
        .store()
        .upsert_messages(&[store::MessageRecord {
            conversation_id: CHAT.into(),
            message_id: "m-edit".into(),
            created_at: base(),
            body_html: "<p>old</p><p><img src=\"https://graph.microsoft.com/v1.0/chats/c/messages/1/hostedContents/9/$value\"></p><attachment id=\"G1\"></attachment>".into(),
            attachments_json: r#"[{"content_type":"reference","name":"a.pdf","url":"https://x/a.pdf","text":null}]"#.into(),
            reactions_json: "[]".into(),
            mentions_json: "[]".into(),
            ..Default::default()
        }])
        .unwrap();

    let _ = engine.edit_message(CHAT, "m-edit", "new").await;

    let call = fake
        .calls()
        .into_iter()
        .find(|call| call.starts_with("edit"))
        .unwrap();
    assert!(call.contains("new<p><img src=\"https://graph.microsoft.com/"));
    let extras = fake.extras_sent.lock().unwrap();
    assert_eq!(extras[0].files[0].attachment_id, "G1");
    assert_eq!(extras[0].files[0].content_url, "https://x/a.pdf");
}

#[tokio::test]
async fn download_streams_every_chunk_and_reports_progress() {
    let fake = Arc::new(Fake::default());
    let engine = chat_engine(&fake, small_pages()).await;
    let written = Mutex::new(0usize);
    let percents = Mutex::new(Vec::new());
    let shared = engine
        .download_file(
            "https://files.example/doc",
            |bytes| {
                *written.lock().unwrap() += bytes.len();
                Ok(())
            },
            |percent| percents.lock().unwrap().push(percent),
        )
        .await
        .unwrap();
    assert_eq!(*written.lock().unwrap() as u64, shared.size);
    let chunk = graph::DOWNLOAD_CHUNK_BYTES;
    let ranges: Vec<String> = fake
        .calls()
        .into_iter()
        .filter(|call| call.starts_with("range"))
        .collect();
    assert_eq!(
        ranges,
        vec![
            format!("range https://dl.example/big 0-{}", chunk - 1),
            format!("range https://dl.example/big {chunk}-{}", 2 * chunk - 1),
            format!("range https://dl.example/big {}-", 2 * chunk),
        ]
    );
    let percents = percents.lock().unwrap();
    assert_eq!(percents.first(), Some(&0));
    assert_eq!(percents.last(), Some(&100));
    assert!(percents.windows(2).all(|pair| pair[0] <= pair[1]));
}

#[tokio::test]
async fn download_keeps_a_file_that_grew_past_the_reported_size() {
    let fake = Arc::new(Fake::default());
    let engine = chat_engine(&fake, small_pages()).await;
    let written = Mutex::new(0usize);
    let percents = Mutex::new(Vec::new());
    let shared = engine
        .download_file(
            "https://files.example/grown",
            |bytes| {
                *written.lock().unwrap() += bytes.len();
                Ok(())
            },
            |percent| percents.lock().unwrap().push(percent),
        )
        .await
        .unwrap();
    assert_eq!(*written.lock().unwrap() as u64, shared.size + 10);
    assert!(
        percents
            .lock()
            .unwrap()
            .iter()
            .all(|percent| *percent <= 100)
    );
}

#[tokio::test]
async fn download_rejects_a_short_chunk() {
    let fake = Arc::new(Fake::default());
    let engine = chat_engine(&fake, small_pages()).await;
    let result = engine
        .download_file("https://files.example/short", |_| Ok(()), |_| {})
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn download_stops_when_the_writer_fails() {
    let fake = Arc::new(Fake::default());
    let engine = chat_engine(&fake, small_pages()).await;
    let result = engine
        .download_file(
            "https://files.example/doc",
            |_| Err(std::io::Error::other("disk full")),
            |_| {},
        )
        .await;
    assert!(matches!(result, Err(Error::Io(_))));
    let ranges = fake
        .calls()
        .into_iter()
        .filter(|call| call.starts_with("range"))
        .count();
    assert_eq!(ranges, 1);
}

#[tokio::test]
async fn upload_to_an_unknown_conversation_fails() {
    let fake = Arc::new(Fake::default());
    let engine = chat_engine(&fake, small_pages()).await;
    assert!(
        engine
            .upload_attachment("unknown", "a.pdf", &[1], |_| {})
            .await
            .is_err()
    );
}

#[tokio::test]
async fn send_with_extras_hands_images_and_files_to_the_remote() {
    let fake = Arc::new(Fake::default());
    let engine = chat_engine(&fake, small_pages()).await;
    let extras = MessageExtras {
        kept: Vec::new(),
        images: vec![HostedImage {
            content_type: "image/png".into(),
            bytes: Arc::new(vec![9]),
        }],
        files: Vec::new(),
    };

    engine
        .send_message_with_extras(CHAT, "look", None, &[], &extras)
        .await
        .unwrap();
    engine.send_message(CHAT, "plain", None).await.unwrap();

    let sent = fake.extras_sent.lock().unwrap();
    assert_eq!(sent[0], extras);
    assert!(sent[1].is_empty());
}

#[tokio::test]
async fn refresh_message_upserts_an_edit_older_than_the_newest_cached() {
    let fake = Arc::new(Fake::default());
    fake.set_chat_messages(numbered(5));
    let engine = chat_engine(&fake, small_pages()).await;
    engine.fetch_newer(CHAT).await.unwrap();
    let mut events = engine.subscribe();

    let mut changed = numbered(5);
    changed[1] = message(2, 2, "<p>old but edited</p>");
    fake.set_chat_messages(changed);
    let record = engine
        .refresh_message(CHAT, "m0002")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.body_html, "<p>old but edited</p>");
    assert_eq!(
        engine.open_conversation(CHAT).unwrap()[1].body_html,
        "<p>old but edited</p>"
    );
    assert_eq!(
        events.try_recv().unwrap(),
        CoreEvent::MessagesChanged {
            conversation_id: CHAT.to_owned()
        }
    );

    assert!(engine.refresh_message(CHAT, "m0999").await.is_err());
}

#[tokio::test]
async fn chat_actions_call_graph_with_the_right_ids() {
    let fake = Arc::new(Fake::default());
    fake.set_chat_messages(numbered(3));
    let engine = chat_engine(&fake, small_pages()).await;
    engine.fetch_newer(CHAT).await.unwrap();

    engine.mark_read(CHAT).await.unwrap();
    engine.set_reaction(CHAT, "m0001", "like").await.unwrap();
    engine.unset_reaction(CHAT, "m0001", "like").await.unwrap();
    engine
        .edit_message(CHAT, "m0002", "new **text**")
        .await
        .unwrap();
    engine.soft_delete_message(CHAT, "m0003").await.unwrap();
    let calls = fake.calls();
    assert_eq!(calls[0], format!("mark_read {CHAT} {ME} tenant-1"));
    assert!(calls.contains(&"set_reaction m0001 like".to_owned()));
    assert!(calls.contains(&"unset_reaction m0001 like".to_owned()));
    assert!(calls.contains(&"edit m0002 new <b>text</b>".to_owned()));
    assert!(calls.contains(&format!("soft_delete {ME} m0003")));
    assert!(!engine.sidebar().unwrap().chats[0].unread);
}

#[tokio::test]
async fn new_chats_are_cached_and_people_are_searched() {
    let fake = Arc::new(Fake::default());
    let engine = engine_with(&fake, SyncConfig::default());
    let one_on_one = engine.create_one_on_one("user-ada").await.unwrap();
    let group = engine
        .create_group(
            &["user-ada".to_owned(), "user-bob".to_owned()],
            Some("Crew"),
        )
        .await
        .unwrap();
    assert_eq!(
        (one_on_one.as_str(), group.as_str()),
        ("19:new@thread.v2", "19:group@thread.v2")
    );
    assert_eq!(engine.store().chat_count().unwrap(), 2);
    assert_eq!(
        fake.calls()[1],
        format!("group {ME} user-ada,user-bob Some(\"Crew\")")
    );
    assert_eq!(engine.search_people("ada").await.unwrap().len(), 1);
}

fn chat_with_preview(
    id: &str,
    minute: i64,
    content_type: &str,
    content: &str,
    deleted: bool,
) -> Chat {
    let mut chat = chat(id, Some("Planning"), minute, 0, "user-ada", false);
    let preview = chat.last_message_preview.as_mut().unwrap();
    preview.body = Some(graph::Body {
        content_type: content_type.to_owned(),
        content: Some(content.to_owned()),
    });
    preview.from =
        serde_json::from_value(json!({"user": {"id": "user-ada", "displayName": "Ada Example"}}))
            .unwrap();
    preview.message_type = Some("message".to_owned());
    if deleted {
        preview.deleted_date_time = Some(base());
    }
    chat
}

#[tokio::test]
async fn sidebar_sync_fills_the_last_message_preview() {
    let fake = Arc::new(Fake::default());
    *fake.chats.lock().unwrap() = vec![
        chat_with_preview(
            "c-html",
            5,
            "html",
            "<p>Hello <b>team</b></p><p>second</p>",
            false,
        ),
        chat_with_preview("c-gone", 4, "html", "<p>secret</p>", true),
        chat_with_preview("c-image", 3, "html", "<p><img src=\"x\"></p>", false),
        chat_with_preview("c-text", 2, "text", &"x".repeat(500), false),
    ];
    let engine = engine_with(&fake, SyncConfig::default());
    engine.refresh_sidebar().await.unwrap();
    let chats = engine.sidebar().unwrap().chats;
    let by_id = |id: &str| chats.iter().find(|chat| chat.id == id).unwrap().clone();
    let html = by_id("c-html");
    assert_eq!(
        html.last_message_preview.as_deref(),
        Some("Hello team second")
    );
    assert_eq!(
        html.last_message_sender_name.as_deref(),
        Some("Ada Example")
    );
    assert_eq!(html.last_message_sender_id.as_deref(), Some("user-ada"));
    assert!(!html.last_message_deleted);
    let gone = by_id("c-gone");
    assert!(gone.last_message_deleted);
    assert_eq!(gone.last_message_preview, None);
    assert_eq!(by_id("c-image").last_message_preview, None);
    assert_eq!(
        by_id("c-text")
            .last_message_preview
            .unwrap()
            .chars()
            .count(),
        200
    );
    assert!(
        html.members
            .iter()
            .all(|member| member.user_id.is_some() && !member.display_name.is_empty())
    );
}

#[tokio::test]
async fn a_newer_message_updates_the_preview_and_announces_the_sidebar() {
    let fake = Arc::new(Fake::default());
    fake.set_chat_messages(numbered(3));
    let engine = chat_engine(&fake, SyncConfig::default()).await;
    engine.fetch_newer(CHAT).await.unwrap();
    let mut events = engine.subscribe();
    let mut all = numbered(3);
    all.push(message(4, 40, "<p>brand <i>new</i></p>"));
    fake.set_chat_messages(all);
    engine.fetch_newer(CHAT).await.unwrap();
    let record = engine.sidebar().unwrap().chats.remove(0);
    assert_eq!(record.last_message_preview.as_deref(), Some("brand new"));
    assert_eq!(
        record.last_message_sender_name.as_deref(),
        Some("Ada Example")
    );
    assert_eq!(record.last_message_at, Some(base() + Duration::minutes(40)));
    let mut seen = Vec::new();
    while let Ok(event) = events.try_recv() {
        seen.push(event);
    }
    assert!(seen.contains(&CoreEvent::SidebarChanged));
}

#[tokio::test]
async fn an_edited_old_message_leaves_the_preview_alone() {
    let fake = Arc::new(Fake::default());
    fake.set_chat_messages(numbered(3));
    let engine = chat_engine(&fake, SyncConfig::default()).await;
    engine.fetch_newer(CHAT).await.unwrap();
    let before = engine.sidebar().unwrap().chats.remove(0);
    let mut all = numbered(3);
    all[0] = message(1, 1, "<p>edited early</p>");
    fake.set_chat_messages(all);
    engine.fetch_newer(CHAT).await.unwrap();
    assert_eq!(engine.sidebar().unwrap().chats.remove(0), before);
}

#[tokio::test]
async fn me_is_known_after_the_first_sync() {
    let fake = Arc::new(Fake::default());
    let engine = engine_with(&fake, SyncConfig::default());
    assert_eq!(engine.me(), None);
    *fake.chats.lock().unwrap() = vec![chat(CHAT, Some("Planning"), 5, 5, ME, false)];
    engine.refresh_sidebar().await.unwrap();
    let me = engine.me().unwrap();
    assert_eq!(me.user_id, ME);
    assert_eq!(me.display_name, "Me Myself");
}

#[tokio::test]
async fn unread_count_uses_cached_messages_after_the_read_time() {
    let fake = Arc::new(Fake::default());
    *fake.chats.lock().unwrap() = vec![chat(CHAT, Some("Planning"), 10, 2, "user-ada", false)];
    fake.set_chat_messages(numbered(5));
    let engine = engine_with(&fake, SyncConfig::default());
    engine.refresh_sidebar().await.unwrap();
    assert_eq!(engine.unread_count(CHAT), 1);
    engine.fetch_newer(CHAT).await.unwrap();
    assert_eq!(engine.unread_count(CHAT), 3);
    assert_eq!(engine.unread_count("unknown"), 0);
    engine.mark_read(CHAT).await.unwrap();
    assert_eq!(engine.unread_count(CHAT), 0);
}

fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

#[tokio::test]
async fn avatars_are_fetched_cached_and_not_refetched_inside_seven_days() {
    let fake = Arc::new(Fake::default());
    fake.photos
        .lock()
        .unwrap()
        .insert("ada".into(), vec![1, 2, 3]);
    let engine = engine_with(&fake, SyncConfig::default());
    let mut events = engine.subscribe();
    engine
        .fetch_avatars(&ids(&["ada", "nobody", "ada"]))
        .await
        .unwrap();
    assert_eq!(
        *fake.photo_requests.lock().unwrap(),
        vec![ids(&["ada", "nobody"])]
    );
    let avatar = engine.avatar("ada").unwrap();
    assert_eq!(avatar.bytes, [1, 2, 3]);
    assert_eq!(avatar.content_type, "image/jpeg");
    assert!(engine.avatar("nobody").is_none());
    assert_eq!(
        events.try_recv().unwrap(),
        CoreEvent::AvatarsChanged {
            user_ids: ids(&["ada"])
        }
    );
    engine
        .fetch_avatars(&ids(&["ada", "nobody"]))
        .await
        .unwrap();
    assert_eq!(fake.photo_requests.lock().unwrap().len(), 1);
    assert!(events.try_recv().is_err());
}

#[tokio::test]
async fn stale_avatars_are_fetched_again() {
    let fake = Arc::new(Fake::default());
    fake.photos.lock().unwrap().insert("ada".into(), vec![9]);
    let engine = engine_with(&fake, SyncConfig::default());
    engine
        .store()
        .upsert_avatar(&store::AvatarRecord {
            user_id: "ada".into(),
            bytes: None,
            content_type: String::new(),
            fetched_at: chrono::Utc::now() - Duration::days(8),
        })
        .unwrap();
    engine.fetch_avatars(&ids(&["ada"])).await.unwrap();
    assert_eq!(engine.avatar("ada").unwrap().bytes, [9]);
}

#[tokio::test]
async fn presence_maps_availability_and_announces_changes_once() {
    let fake = Arc::new(Fake::default());
    let entry = |user_id: &str, availability: &str| Presence {
        user_id: user_id.to_owned(),
        availability: availability.to_owned(),
        activity: None,
    };
    *fake.presence_answers.lock().unwrap() = vec![
        entry("ada", "BusyIdle"),
        entry("bob", "BeRightBack"),
        entry("cy", "PresenceUnknown"),
        entry("di", "DoNotDisturb"),
    ];
    let engine = engine_with(&fake, SyncConfig::default());
    let mut events = engine.subscribe();
    assert!(engine.presence("ada").is_none());
    let wanted = ids(&["ada", "bob", "cy", "di"]);
    engine.refresh_presence(&wanted).await.unwrap();
    assert_eq!(
        engine.presence("ada").unwrap().availability,
        Availability::Busy
    );
    assert_eq!(
        engine.presence("bob").unwrap().availability,
        Availability::Away
    );
    assert_eq!(
        engine.presence("cy").unwrap().availability,
        Availability::Unknown
    );
    assert_eq!(
        engine.presence("di").unwrap().availability,
        Availability::DoNotDisturb
    );
    assert_eq!(events.try_recv().unwrap(), CoreEvent::PresenceChanged);
    engine.refresh_presence(&wanted).await.unwrap();
    assert!(events.try_recv().is_err());
}

struct FakeFolders {
    state: Mutex<Vec<ChatFolder>>,
    channels: Vec<String>,
    layout: Vec<TeamLayoutRecord>,
    calls: Mutex<Vec<String>>,
}

impl FakeFolders {
    fn new() -> Arc<Self> {
        let folder = |id: &str, kind, items: &[&str]| ChatFolder {
            id: id.to_owned(),
            name: id.to_owned(),
            kind,
            conversation_ids: ids(items),
        };
        Arc::new(FakeFolders {
            state: Mutex::new(vec![
                folder("fav", FolderKind::Favorites, &["a", "b"]),
                folder("work", FolderKind::UserCreated, &["c"]),
            ]),
            channels: ids(&["ch1", "ch2"]),
            layout: Vec::new(),
            calls: Mutex::new(Vec::new()),
        })
    }
}

impl FolderSource for FakeFolders {
    fn folders(&self) -> BoxFuture<'_, Result<Vec<ChatFolder>>> {
        Box::pin(async { Ok(self.state.lock().unwrap().clone()) })
    }

    fn pinned_channels(&self) -> BoxFuture<'_, Result<Vec<String>>> {
        Box::pin(async { Ok(self.channels.clone()) })
    }

    fn team_layout(&self) -> BoxFuture<'_, Result<Vec<TeamLayoutRecord>>> {
        Box::pin(async { Ok(self.layout.clone()) })
    }

    fn move_to_folder<'a>(
        &'a self,
        conversation_id: &'a str,
        target_folder_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<ChatFolder>>> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push(format!("move {conversation_id} {target_folder_id}"));
            let mut state = self.state.lock().unwrap();
            for folder in state
                .iter_mut()
                .filter(|folder| folder.kind == FolderKind::UserCreated)
            {
                folder.conversation_ids.retain(|id| id != conversation_id);
            }
            state
                .iter_mut()
                .find(|folder| folder.id == target_folder_id)
                .unwrap()
                .conversation_ids
                .push(conversation_id.to_owned());
            Ok(state.clone())
        })
    }

    fn remove_from_folder<'a>(
        &'a self,
        conversation_id: &'a str,
        folder_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<ChatFolder>>> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push(format!("remove {conversation_id} {folder_id}"));
            let mut state = self.state.lock().unwrap();
            state
                .iter_mut()
                .find(|folder| folder.id == folder_id)
                .unwrap()
                .conversation_ids
                .retain(|id| id != conversation_id);
            Ok(state.clone())
        })
    }
}

#[tokio::test]
async fn folders_refresh_move_and_remove_go_through_the_source_and_cache() {
    let fake = Arc::new(Fake::default());
    let source = FakeFolders::new();
    let engine = engine_with(&fake, SyncConfig::default()).with_folder_source(source.clone());
    assert!(engine.chat_folders().unwrap().is_empty());
    let mut events = engine.subscribe();

    engine.refresh_folders().await.unwrap();
    let folders = engine.chat_folders().unwrap();
    assert_eq!(folders.len(), 2);
    assert_eq!(folders[0].kind, FolderKind::Favorites);
    assert_eq!(folders[0].conversation_ids, ids(&["a", "b"]));
    assert_eq!(engine.pinned_channels().unwrap(), ids(&["ch1", "ch2"]));
    assert_eq!(events.try_recv().unwrap(), CoreEvent::FoldersChanged);

    engine.refresh_folders().await.unwrap();
    assert!(events.try_recv().is_err());

    engine.move_to_folder("a", "work").await.unwrap();
    assert_eq!(
        engine.chat_folders().unwrap()[1].conversation_ids,
        ids(&["c", "a"])
    );
    assert_eq!(events.try_recv().unwrap(), CoreEvent::FoldersChanged);
    assert_eq!(engine.pinned_channels().unwrap(), ids(&["ch1", "ch2"]));

    engine.remove_from_folder("a", "work").await.unwrap();
    assert_eq!(
        engine.chat_folders().unwrap()[1].conversation_ids,
        ids(&["c"])
    );
    assert_eq!(
        *source.calls.lock().unwrap(),
        ["move a work", "remove a work"]
    );
}

#[tokio::test]
async fn team_layout_from_teams_orders_and_hides_the_sidebar() {
    let fake = Arc::new(Fake::default());
    let mut source = FakeFolders::new();
    Arc::get_mut(&mut source).unwrap().layout = vec![
        TeamLayoutRecord {
            team_id: "zeta".into(),
            hidden: false,
            channels: vec![ChannelLayoutRecord {
                channel_id: "zeta-old".into(),
                general: false,
                hidden: true,
            }],
        },
        TeamLayoutRecord {
            team_id: "alpha".into(),
            hidden: true,
            channels: Vec::new(),
        },
    ];
    let engine = engine_with(&fake, SyncConfig::default()).with_folder_source(source.clone());
    let team = |id: &str| TeamRecord {
        id: id.into(),
        name: id.into(),
    };
    engine
        .store()
        .upsert_teams(&[team("alpha"), team("zeta")])
        .unwrap();
    let channel = |id: &str, name: &str| ChannelRecord {
        id: id.into(),
        team_id: "zeta".into(),
        name: name.into(),
        membership_type: None,
        last_message_at: None,
        unread: false,
    };
    engine
        .store()
        .upsert_channels(&[
            channel("zeta-old", "Archiv"),
            channel("zeta-general", "General"),
        ])
        .unwrap();
    let mut events = engine.subscribe();

    engine.refresh_folders().await.unwrap();

    let teams = engine.store().sidebar().unwrap().teams;
    let order: Vec<&str> = teams.iter().map(|entry| entry.team.id.as_str()).collect();
    assert_eq!(order, ["zeta", "alpha"]);
    assert!(teams[1].hidden);
    assert_eq!(teams[0].channels[0].id, "zeta-general");
    assert_eq!(teams[0].hidden_channel_ids, ids(&["zeta-old"]));
    let received: Vec<CoreEvent> = std::iter::from_fn(|| events.try_recv().ok()).collect();
    assert!(received.contains(&CoreEvent::SidebarChanged));
}

#[tokio::test]
async fn folders_without_a_source_are_unsupported() {
    let fake = Arc::new(Fake::default());
    let engine = engine_with(&fake, SyncConfig::default());
    assert!(matches!(
        engine.refresh_folders().await,
        Err(Error::Unsupported(_))
    ));
    assert!(matches!(
        engine.move_to_folder("a", "b").await,
        Err(Error::Unsupported(_))
    ));
}

const HOSTED: &str =
    "https://graph.microsoft.com/v1.0/chats/c/messages/m0001/hostedContents/h1/$value";

fn message_with_image(id: u32, minute: i64, url: &str) -> Message {
    message(
        id,
        minute,
        &format!("<p>look</p><img src=\"{url}\" width=\"300\" height=\"150\">"),
    )
}

#[tokio::test]
async fn images_are_fetched_once_cached_and_announced() {
    let fake = Arc::new(Fake::default());
    fake.set_chat_messages(vec![message_with_image(1, 1, HOSTED)]);
    let engine = chat_engine(&fake, small_pages()).await;
    engine.fetch_newer(CHAT).await.unwrap();
    let record = engine.open_conversation(CHAT).unwrap().remove(0);
    let found = teams_core::images(&record);
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].width, found[0].height), (Some(300), Some(150)));
    assert_eq!(found[0].id, "h1");
    assert!(engine.image(found[0].key()).is_none());

    let mut events = engine.subscribe();
    engine.fetch_image(&found[0]).await.unwrap();
    engine.fetch_image(&found[0]).await.unwrap();
    assert_eq!(fake.hosted_requests.lock().unwrap().len(), 1);
    assert_eq!(
        events.try_recv().unwrap(),
        CoreEvent::ImagesChanged {
            keys: vec![HOSTED.to_owned()]
        }
    );
    let stored = engine.image(found[0].key()).unwrap();
    assert_eq!(stored.content_type, "image/png");
    assert_eq!((stored.width, stored.height), (Some(40), Some(20)));
}

#[tokio::test]
async fn a_failed_image_fetch_is_an_error_and_can_be_retried() {
    let fake = Arc::new(Fake::default());
    let engine = chat_engine(&fake, small_pages()).await;
    let image = teams_core::ImageRef {
        id: "x".to_owned(),
        url: "https://graph.microsoft.com/v1.0/chats/broken".to_owned(),
        width: None,
        height: None,
    };
    assert!(engine.fetch_image(&image).await.is_err());
    assert!(engine.fetch_image(&image).await.is_err());
    assert_eq!(fake.hosted_requests.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn local_search_finds_cached_messages_and_titles() {
    let fake = Arc::new(Fake::default());
    fake.set_chat_messages(vec![
        message(1, 1, "<p>quarterly <b>budget</b> review</p>"),
        message(2, 2, "<p>lunch?</p>"),
    ]);
    let engine = chat_engine(&fake, small_pages()).await;
    engine.fetch_newer(CHAT).await.unwrap();
    let hits = engine.search_messages("budg", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(
        (
            hits[0].conversation_id.as_str(),
            hits[0].message_id.as_str()
        ),
        (CHAT, "m0001")
    );
    assert!(hits[0].snippet.contains("\u{1}budget\u{2}"));
    assert_eq!(
        engine.search_messages_in(CHAT, "lunch", 10).unwrap().len(),
        1
    );
    assert!(
        engine
            .search_messages_in("other", "lunch", 10)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        engine.search_conversations("plann", 10).unwrap()[0].conversation_id,
        CHAT
    );
}

#[tokio::test]
async fn first_unread_is_the_first_message_from_others_after_the_read_time() {
    let fake = Arc::new(Fake::default());
    *fake.chats.lock().unwrap() = vec![chat(CHAT, Some("Planning"), 10, 2, "user-ada", false)];
    fake.set_chat_messages(numbered(5));
    let engine = engine_with(&fake, SyncConfig::default());
    engine.refresh_sidebar().await.unwrap();
    engine.fetch_newer(CHAT).await.unwrap();
    assert_eq!(
        engine.first_unread_message_id(CHAT).as_deref(),
        Some("m0003")
    );
    assert!(engine.first_unread_message_id("unknown").is_none());
    engine.mark_read(CHAT).await.unwrap();
    assert!(engine.first_unread_message_id(CHAT).is_none());
}

#[tokio::test]
async fn chat_reply_quotes_and_channel_reply_uses_the_thread_root() {
    let fake = Arc::new(Fake::default());
    fake.set_chat_messages(numbered(2));
    let engine = chat_engine(&fake, small_pages()).await;
    engine.fetch_newer(CHAT).await.unwrap();
    let sent = engine
        .reply_to(CHAT, "m0001", "answer **bold**")
        .await
        .unwrap();
    assert_eq!(sent.message_id, "m9003");
    assert!(
        fake.calls()
            .contains(&format!("quote_reply {CHAT} m0001 answer <b>bold</b>"))
    );

    let channel_fake = Arc::new(Fake::default());
    let channel = channel_engine(&channel_fake).await;
    channel.fetch_newer(CHANNEL).await.unwrap();
    channel.reply_to(CHANNEL, "m0030", "hi").await.unwrap();
    assert!(channel_fake.calls().contains(&"reply to m0030".to_owned()));
    assert!(engine.reply_to("unknown", "m1", "x").await.is_err());
}

#[tokio::test]
async fn can_edit_and_delete_need_an_own_live_chat_message() {
    let fake = Arc::new(Fake::default());
    let mut own = message(1, 1, "<p>mine</p>");
    own.from =
        serde_json::from_value(json!({"user": {"id": ME, "displayName": "Me Myself"}})).unwrap();
    fake.set_chat_messages(vec![own, message(2, 2, "<p>theirs</p>")]);
    let engine = chat_engine(&fake, small_pages()).await;
    engine.fetch_newer(CHAT).await.unwrap();
    let records = engine.open_conversation(CHAT).unwrap();
    assert!(engine.can_edit(&records[0]) && engine.can_delete(&records[0]));
    assert!(!engine.can_edit(&records[1]) && !engine.can_delete(&records[1]));
    let mut deleted = records[0].clone();
    deleted.deleted = true;
    assert!(!engine.can_edit(&deleted));
    assert!(teams_core::can_edit(&records[0], ME));
    assert!(!teams_core::can_delete(&deleted, ME));
}

#[tokio::test]
async fn channel_posts_and_replies_can_be_reacted_edited_and_deleted() {
    let fake = Arc::new(Fake::default());
    let engine = channel_engine(&fake).await;
    engine.fetch_newer(CHANNEL).await.unwrap();
    let reply_id = engine
        .open_conversation(CHANNEL)
        .unwrap()
        .into_iter()
        .find(|record| record.reply_to_id.is_some())
        .unwrap()
        .message_id;

    engine.set_reaction(CHANNEL, "m0030", "like").await.unwrap();
    engine
        .unset_reaction(CHANNEL, &reply_id, "like")
        .await
        .unwrap();
    engine
        .edit_message(CHANNEL, &reply_id, "new **text**")
        .await
        .unwrap();
    engine.edit_message(CHANNEL, "m0030", "root").await.unwrap();
    engine
        .soft_delete_message(CHANNEL, &reply_id)
        .await
        .unwrap();
    engine.soft_delete_message(CHANNEL, "m0030").await.unwrap();

    let calls = fake.calls();
    assert!(calls.contains(&"set_reaction channel:-:m0030 like".to_owned()));
    assert!(calls.contains(&format!("unset_reaction channel:m0030:{reply_id} like")));
    assert!(calls.contains(&format!("edit channel:m0030:{reply_id} new <b>text</b>")));
    assert!(calls.contains(&"edit channel:-:m0030 root".to_owned()));
    assert!(calls.contains(&format!("soft_delete {ME} channel:m0030:{reply_id}")));
    assert!(calls.contains(&format!("soft_delete {ME} channel:-:m0030")));
    assert!(engine.set_reaction("unknown", "m1", "like").await.is_err());
}

#[tokio::test]
async fn channel_messages_of_mine_are_editable_and_deletable() {
    let fake = Arc::new(Fake::default());
    let engine = channel_engine(&fake).await;
    engine.fetch_newer(CHANNEL).await.unwrap();
    let mut record = engine.open_conversation(CHANNEL).unwrap().remove(0);
    assert!(!engine.can_edit(&record));
    record.sender_id = Some(ME.to_owned());
    assert!(engine.can_edit(&record) && engine.can_delete(&record));
}

#[tokio::test]
async fn chat_mentions_become_at_tags_and_a_mentions_array() {
    let fake = Arc::new(Fake::default());
    let engine = chat_engine(&fake, small_pages()).await;
    engine
        .send_message_with_mentions(
            CHAT,
            "hi @Ada Example & co",
            None,
            &[MentionInput::user("user-ada", "Ada Example")],
        )
        .await
        .unwrap();
    assert_eq!(
        fake.sent.lock().unwrap()[0],
        "hi <at id=\"0\">Ada Example</at> &amp; co"
    );
    let mentions = fake.mentions_sent.lock().unwrap()[0].clone();
    assert_eq!(mentions.len(), 1);
    assert_eq!(mentions[0].id, 0);
    assert_eq!(
        mentions[0].target,
        MentionTarget::User {
            user_id: "user-ada".to_owned()
        }
    );
    assert!(
        engine
            .send_message_with_mentions(CHAT, "@Squad", None, &[MentionInput::team("t", "Squad")])
            .await
            .is_err()
    );
}

#[tokio::test]
async fn channel_mentions_cover_users_channel_and_team_on_post_reply_and_edit() {
    let fake = Arc::new(Fake::default());
    let engine = channel_engine(&fake).await;
    engine.fetch_newer(CHANNEL).await.unwrap();
    let everyone = [
        MentionInput::user("user-ada", "Ada Example"),
        MentionInput::channel(CHANNEL, "General"),
        MentionInput::team("team-1", "Squad"),
    ];
    let text = "@Ada Example @General @Squad";
    engine
        .post_to_channel_with_mentions(CHANNEL, text, None, &everyone)
        .await
        .unwrap();
    engine
        .reply_to_with_mentions(CHANNEL, "m0030", text, &everyone)
        .await
        .unwrap();
    engine
        .edit_message_with_mentions(CHANNEL, "m0030", text, &everyone)
        .await
        .unwrap();
    for mentions in fake.mentions_sent.lock().unwrap().iter() {
        assert_eq!(mentions.len(), 3);
        assert_eq!(
            mentions
                .iter()
                .map(|mention| mention.id)
                .collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert!(matches!(mentions[1].target, MentionTarget::Channel { .. }));
        assert!(matches!(mentions[2].target, MentionTarget::Team { .. }));
    }
    assert_eq!(fake.mentions_sent.lock().unwrap().len(), 3);
}

fn directory_user(id: &str, name: &str) -> User {
    serde_json::from_value(
        json!({"id": id, "displayName": name, "mail": format!("{id}@example.test")}),
    )
    .unwrap()
}

#[tokio::test]
async fn people_candidates_list_members_first_then_the_directory_without_duplicates() {
    let fake = Arc::new(Fake::default());
    *fake.directory_users.lock().unwrap() = vec![
        directory_user("user-me", "Me Myself"),
        directory_user("user-ada", "Ada Example"),
        directory_user("user-adam", "Adam Directory"),
    ];
    let engine = chat_engine(&fake, small_pages()).await;

    let everyone = engine.mention_candidates(CHAT, "", 10).await.unwrap();
    assert_eq!(everyone.len(), 1);
    assert_eq!(everyone[0].display_name(), "Ada Example");
    assert!(!fake.calls().iter().any(|call| call.starts_with("search")));

    let found = engine.mention_candidates(CHAT, "@ada", 10).await.unwrap();
    let names: Vec<&str> = found
        .iter()
        .map(|candidate| candidate.display_name())
        .collect();
    assert_eq!(names, ["Ada Example", "Adam Directory"]);
    assert!(
        matches!(&found[0], MentionCandidate::Person(person) if person.source == PersonSource::Member)
    );
    assert!(
        matches!(&found[1], MentionCandidate::Person(person) if person.source == PersonSource::Directory)
    );

    engine.mention_candidates(CHAT, "ada", 10).await.unwrap();
    engine.search_people("ADA").await.unwrap();
    let searches = fake
        .calls()
        .into_iter()
        .filter(|call| call.starts_with("search"))
        .count();
    assert_eq!(searches, 1);

    assert_eq!(
        engine
            .mention_candidates(CHAT, "ada", 1)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(engine.mention_candidates("unknown", "a", 5).await.is_err());
}

#[tokio::test]
async fn channel_candidates_offer_recent_senders_the_channel_and_its_team() {
    let fake = Arc::new(Fake::default());
    let engine = channel_engine(&fake).await;
    engine.fetch_newer(CHANNEL).await.unwrap();
    let all = engine.mention_candidates(CHANNEL, "", 10).await.unwrap();
    assert_eq!(all.len(), 3);
    assert!(matches!(&all[0], MentionCandidate::Person(person) if person.user_id == "user-ada"));
    assert!(
        matches!(&all[1], MentionCandidate::Channel { channel_id, .. } if channel_id == CHANNEL)
    );
    assert!(matches!(&all[2], MentionCandidate::Team { team_id, .. } if team_id == "team-1"));
    let squad = engine.mention_candidates(CHANNEL, "squ", 10).await.unwrap();
    assert_eq!(squad[0].to_mention(), MentionInput::team("team-1", "Squad"));
}

#[tokio::test]
async fn images_land_as_lru_files_and_are_served_by_path() {
    let directory = tempfile::tempdir().unwrap();
    let fake = Arc::new(Fake::default());
    *fake.chats.lock().unwrap() = vec![chat(CHAT, Some("Planning"), 5, 5, ME, false)];
    let engine = engine_with(&fake, small_pages())
        .with_image_dir(directory.path(), 1024 * 1024)
        .unwrap();
    engine.refresh_sidebar().await.unwrap();
    let image = teams_core::ImageRef {
        id: "h1".to_owned(),
        url: HOSTED.to_owned(),
        width: None,
        height: None,
    };
    assert!(engine.image_path(&image).is_none());
    let mut events = engine.subscribe();
    let path = engine.fetch_image(&image).await.unwrap().unwrap();
    assert_eq!(path.extension().unwrap(), "png");
    assert!(path.starts_with(directory.path()));
    assert_eq!(engine.image_path(&image), Some(path.clone()));
    assert_eq!(engine.fetch_image(&image).await.unwrap(), Some(path));
    assert_eq!(fake.hosted_requests.lock().unwrap().len(), 1);
    assert!(matches!(
        events.try_recv().unwrap(),
        CoreEvent::ImagesChanged { .. }
    ));
    let stored = engine.image(HOSTED).unwrap();
    assert_eq!((stored.width, stored.height), (Some(40), Some(20)));
    assert!(engine.store().image(HOSTED).unwrap().is_none());
}

#[tokio::test]
async fn a_read_chat_has_no_first_unread_message() {
    let fake = Arc::new(Fake::default());
    *fake.chats.lock().unwrap() = vec![chat(CHAT, Some("Planning"), 10, 10, "user-ada", false)];
    fake.set_chat_messages(numbered(5));
    let engine = engine_with(&fake, SyncConfig::default());
    engine.refresh_sidebar().await.unwrap();
    engine.fetch_newer(CHAT).await.unwrap();
    assert!(engine.first_unread_message_id(CHAT).is_none());
}

fn millis(minute: i64) -> i64 {
    (base() + Duration::minutes(minute)).timestamp_millis()
}

fn horizon_of(user: &str, read_minute: i64, at_minute: i64) -> chatsvc::MemberHorizon {
    chatsvc::receipts::parse_horizon(
        &format!("8:orgid:{user}"),
        &format!(
            "{};{};6638000000000000001",
            millis(read_minute),
            millis(at_minute)
        ),
    )
    .unwrap()
}

fn own_message(id: u32, minute: i64) -> Message {
    serde_json::from_value(json!({
        "id": format!("m{id:04}"),
        "messageType": "message",
        "createdDateTime": (base() + Duration::minutes(minute)).to_rfc3339(),
        "from": {"user": {"id": ME, "displayName": "Me Myself"}},
        "body": {"contentType": "html", "content": "<p>mine</p>"},
    }))
    .unwrap()
}

fn group_chat(id: &str) -> Chat {
    serde_json::from_value(json!({
        "id": id,
        "topic": "Planning",
        "chatType": "group",
        "lastUpdatedDateTime": base().to_rfc3339(),
        "members": [
            {"userId": ME, "displayName": "Me Myself"},
            {"userId": "user-ada", "displayName": "Ada Example"},
            {"userId": "user-bob", "displayName": "Bob Example"},
            {"userId": "user-cy", "displayName": "Cy Example"}
        ],
        "viewpoint": {"lastMessageReadDateTime": base().to_rfc3339(), "isHidden": false},
    }))
    .unwrap()
}

async fn receipt_engine(fake: &Arc<Fake>, chats: Vec<Chat>) -> SyncEngine<Handle> {
    *fake.chats.lock().unwrap() = chats;
    fake.set_chat_messages(vec![
        own_message(1, 1),
        message(2, 2, "<p>theirs</p>"),
        own_message(3, 3),
    ]);
    let engine = engine_with(fake, small_pages())
        .with_receipt_debounce(std::time::Duration::from_millis(60));
    engine.refresh_sidebar().await.unwrap();
    engine.fetch_newer(CHAT).await.unwrap();
    engine
}

fn one_on_one() -> Chat {
    chat(CHAT, None, 5, 5, ME, false)
}

#[tokio::test]
async fn one_on_one_read_up_to_horizon() {
    let fake = Arc::new(Fake::default());
    let engine = receipt_engine(&fake, vec![one_on_one()]).await;
    *fake.horizons.lock().unwrap() =
        vec![horizon_of("user-me", 3, 3), horizon_of("user-ada", 2, 9)];
    let mut events = engine.subscribe();
    engine.refresh_receipts(CHAT).await.unwrap();
    assert_eq!(
        events.try_recv().unwrap(),
        CoreEvent::ReceiptsChanged {
            conversation_id: CHAT.into()
        }
    );
    assert_eq!(
        engine.receipt_state(CHAT, "m0001"),
        teams_core::ReceiptState::Read {
            readers: vec![teams_core::ReceiptReader {
                name: "Ada Example".into(),
                at: base() + Duration::minutes(9),
            }],
            total_others: 1,
        }
    );
    assert_eq!(
        engine.receipt_state(CHAT, "m0003"),
        teams_core::ReceiptState::Sent
    );
    assert_eq!(
        engine.receipt_state(CHAT, "m0002"),
        teams_core::ReceiptState::Unknown
    );
    assert_eq!(
        engine.receipt_state(CHAT, "missing"),
        teams_core::ReceiptState::Unknown
    );
}

#[tokio::test]
async fn horizon_equal_to_creation_time_counts_as_read() {
    let fake = Arc::new(Fake::default());
    let engine = receipt_engine(&fake, vec![one_on_one()]).await;
    *fake.horizons.lock().unwrap() = vec![horizon_of("user-ada", 3, 3)];
    engine.refresh_receipts(CHAT).await.unwrap();
    assert!(matches!(
        engine.receipt_state(CHAT, "m0003"),
        teams_core::ReceiptState::Read { .. }
    ));
}

#[tokio::test]
async fn nobody_read_means_sent() {
    let fake = Arc::new(Fake::default());
    let engine = receipt_engine(&fake, vec![one_on_one()]).await;
    *fake.horizons.lock().unwrap() =
        vec![horizon_of("user-me", 3, 3), horizon_of("user-ada", 0, 0)];
    engine.refresh_receipts(CHAT).await.unwrap();
    assert_eq!(
        engine.receipt_state(CHAT, "m0001"),
        teams_core::ReceiptState::Sent
    );
    assert_eq!(
        engine.receipt_state(CHAT, "m0003"),
        teams_core::ReceiptState::Sent
    );
}

#[tokio::test]
async fn group_partial_read_lists_who_and_total() {
    let fake = Arc::new(Fake::default());
    let engine = receipt_engine(&fake, vec![group_chat(CHAT)]).await;
    *fake.horizons.lock().unwrap() = vec![
        horizon_of("user-me", 3, 3),
        horizon_of("user-cy", 3, 8),
        horizon_of("user-ada", 1, 6),
        horizon_of("user-bob", 0, 0),
    ];
    engine.refresh_receipts(CHAT).await.unwrap();
    let names = |state: teams_core::ReceiptState| match state {
        teams_core::ReceiptState::Read {
            readers,
            total_others,
        } => (
            readers
                .into_iter()
                .map(|reader| reader.name)
                .collect::<Vec<_>>(),
            total_others,
        ),
        other => panic!("expected read, got {other:?}"),
    };
    assert_eq!(
        names(engine.receipt_state(CHAT, "m0001")),
        (vec!["Ada Example".to_owned(), "Cy Example".to_owned()], 3)
    );
    assert_eq!(
        names(engine.receipt_state(CHAT, "m0003")),
        (vec!["Cy Example".to_owned()], 3)
    );
}

#[tokio::test]
async fn disabled_receipts_are_unknown() {
    let fake = Arc::new(Fake::default());
    let engine = receipt_engine(&fake, vec![one_on_one()]).await;
    engine.refresh_receipts(CHAT).await.unwrap();
    assert_eq!(
        engine.receipt_state(CHAT, "m0001"),
        teams_core::ReceiptState::Unknown
    );
    *fake.horizons.lock().unwrap() = vec![horizon_of("user-me", 3, 3)];
    engine.refresh_receipts(CHAT).await.unwrap();
    assert_eq!(
        engine.receipt_state(CHAT, "m0001"),
        teams_core::ReceiptState::Unknown
    );
}

#[tokio::test]
async fn disabling_after_reading_clears_the_state_and_announces() {
    let fake = Arc::new(Fake::default());
    let engine = receipt_engine(&fake, vec![one_on_one()]).await;
    *fake.horizons.lock().unwrap() = vec![horizon_of("user-ada", 3, 3)];
    engine.refresh_receipts(CHAT).await.unwrap();
    let mut events = engine.subscribe();
    fake.horizons.lock().unwrap().clear();
    engine.refresh_receipts(CHAT).await.unwrap();
    assert_eq!(
        events.try_recv().unwrap(),
        CoreEvent::ReceiptsChanged {
            conversation_id: CHAT.into()
        }
    );
    assert_eq!(
        engine.receipt_state(CHAT, "m0001"),
        teams_core::ReceiptState::Unknown
    );
}

#[tokio::test]
async fn unchanged_horizons_announce_nothing() {
    let fake = Arc::new(Fake::default());
    let engine = receipt_engine(&fake, vec![one_on_one()]).await;
    *fake.horizons.lock().unwrap() = vec![horizon_of("user-ada", 3, 3)];
    engine.refresh_receipts(CHAT).await.unwrap();
    let mut events = engine.subscribe();
    engine.refresh_receipts(CHAT).await.unwrap();
    assert!(events.try_recv().is_err());
}

#[tokio::test]
async fn channels_and_deleted_messages_have_no_state() {
    let fake = Arc::new(Fake::default());
    let engine = receipt_engine(&fake, vec![one_on_one()]).await;
    *fake.horizons.lock().unwrap() = vec![horizon_of("user-ada", 3, 3)];
    engine.refresh_receipts(CHAT).await.unwrap();
    let mut deleted = engine.open_conversation(CHAT).unwrap().remove(0);
    deleted.deleted = true;
    assert_eq!(
        engine.receipt_state_for(&deleted),
        teams_core::ReceiptState::Unknown
    );
    engine.refresh_receipts(CHANNEL).await.unwrap_err();
    assert_eq!(*fake.horizon_calls.lock().unwrap(), 1);
}

#[tokio::test]
async fn read_event_burst_triggers_one_fetch() {
    let fake = Arc::new(Fake::default());
    let engine = receipt_engine(&fake, vec![one_on_one()]).await;
    *fake.horizons.lock().unwrap() = vec![horizon_of("user-ada", 1, 4)];
    engine.refresh_receipts(CHAT).await.unwrap();
    let before = *fake.horizon_calls.lock().unwrap();
    *fake.horizons.lock().unwrap() = vec![horizon_of("user-ada", 3, 7)];
    let event = read_event(Some(CHAT));
    let started = std::time::Instant::now();
    tokio::join!(
        engine.handle_receipt_event(&event),
        engine.handle_receipt_event(&event),
        engine.handle_receipt_event(&event),
    );
    assert!(started.elapsed() >= std::time::Duration::from_millis(60));
    assert_eq!(*fake.horizon_calls.lock().unwrap(), before + 1);
    assert!(matches!(
        engine.receipt_state(CHAT, "m0003"),
        teams_core::ReceiptState::Read { .. }
    ));
}

fn read_event(conversation_id: Option<&str>) -> chatsvc::MessageEvent {
    chatsvc::MessageEvent {
        resource_type: "NewMessage".into(),
        kind: chatsvc::EventKind::ReadReceipt,
        conversation_id: conversation_id.map(str::to_owned),
        message_id: None,
        received_at: base(),
    }
}

#[tokio::test]
async fn events_for_other_kinds_or_unopened_chats_do_nothing() {
    let fake = Arc::new(Fake::default());
    let engine = receipt_engine(&fake, vec![one_on_one()]).await;
    let mut typing = read_event(Some(CHAT));
    typing.kind = chatsvc::EventKind::Typing;
    engine.handle_receipt_event(&typing).await;
    engine.handle_receipt_event(&read_event(Some(CHAT))).await;
    engine.handle_receipt_event(&read_event(None)).await;
    assert_eq!(*fake.horizon_calls.lock().unwrap(), 0);
}

#[tokio::test]
async fn reconnect_refetches_known_chats_only() {
    let fake = Arc::new(Fake::default());
    let engine = receipt_engine(&fake, vec![one_on_one()]).await;
    let status = |kind| chatsvc::StatusEvent {
        kind,
        detail: String::new(),
    };
    engine
        .handle_receipt_status(&status(chatsvc::StatusKind::Connected))
        .await;
    assert_eq!(*fake.horizon_calls.lock().unwrap(), 0);
    *fake.horizons.lock().unwrap() = vec![horizon_of("user-ada", 1, 4)];
    engine.refresh_receipts(CHAT).await.unwrap();
    engine
        .handle_receipt_status(&status(chatsvc::StatusKind::Disconnected))
        .await;
    assert_eq!(*fake.horizon_calls.lock().unwrap(), 1);
    *fake.horizons.lock().unwrap() = vec![horizon_of("user-ada", 3, 7)];
    let mut events = engine.subscribe();
    engine
        .handle_receipt_status(&status(chatsvc::StatusKind::MessageLoss))
        .await;
    assert_eq!(*fake.horizon_calls.lock().unwrap(), 2);
    assert!(matches!(
        events.try_recv().unwrap(),
        CoreEvent::ReceiptsChanged { .. }
    ));
}
