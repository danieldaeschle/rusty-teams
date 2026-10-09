use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use chrono::{DateTime, Duration, Local, TimeZone, Utc};
use gpui_kit::Image;
use store::{
    ChannelLayoutRecord, ChannelRecord, ChatRecord, MemberRecord, MessageRecord, Store,
    TeamLayoutRecord, TeamRecord,
};
use teams_core::{
    CardActionOutcome, ChatApp, Gif, LinkPreview, MentionCandidate, PersonCandidate, PersonSource,
    PinnedMessage, SavedMessage, TaskDialog, TaskDialogKind,
};

use crate::activity::{Actor, Entry, Kind};
use crate::app_state::{AppState, Selection};
use crate::card_actions::{CardAnswer, CardTask};
use crate::data::{FolderInfo, FolderKind, Person, PresenceKind};
use crate::demo_gifs;
use crate::message_actions::ForwardSource;

static SEARCHABLE_GIFS: OnceLock<Vec<Gif>> = OnceLock::new();

pub const DEMO_USER_ID: &str = "demo-me";
const DEMO_USER_NAME: &str = "Dana Demo";
const DEMO_USER_EMAIL: &str = "dana.demo@example.com";
const PHOTO_SIZE: usize = 48;
const STAGING_IMAGE_KEY: &str = "demo://staging-dashboard";
const PENDING_IMAGE_KEY: &str = "demo://pending-screenshot";
const STAGING_SIZE: (usize, usize) = (960, 540);
const LINK_IMAGE_KEY: &str =
    "https://de-prod.asyncgw.teams.microsoft.com/urlp/v1/url/image/Thumbnail?url=demo";
const LINK_IMAGE_SIZE: (u32, u32) = (320, 180);
const PRIORITY_CHAT: &str = "demo-chat-priya";
const BOT_CHAT: &str = "demo-chat-wiki-bot";
const BOT_NAME: &str = "Wiki Bot";
const BOT_APP_NAME: &str = "Wiki Cloud";
const BOT_ID: &str = "00000000-0000-0000-0000-00000000b07a";
const CARD_ANSWER_DELAY: std::time::Duration = std::time::Duration::from_millis(700);
const CARD_PERSON_URL: &str = "https://avatars.githubusercontent.com/u/9919?s=64";
const CARD_PAGE_URL: &str = "https://avatars.githubusercontent.com/u/9919?s=64";
const NEW_CHAT_PREFIX: &str = "demo-chat-new-";
const PEOPLE_DIRECTORY: [(&str, &str, &str, &str); 5] = [
    (
        MARA_ID,
        "Mara Lindqvist",
        "Product Owner",
        "mara.lindqvist@example.com",
    ),
    (
        JONAS_ID,
        "Jonas Ortega",
        "Backend Engineer",
        "jonas.ortega@example.com",
    ),
    (
        PRIYA_ID,
        "Priya Nair",
        "Engineering Manager",
        "priya.nair@example.com",
    ),
    (
        LEA_ID,
        "Lea Schneider",
        "Customer Success",
        "lea.schneider@example.com",
    ),
    (
        TOBIAS_ID,
        "Tobias Klein",
        "Support Engineer",
        "tobias.klein@example.com",
    ),
];

struct Palette {
    background: [u8; 3],
    skin: [u8; 3],
    hair: [u8; 3],
    shirt: [u8; 3],
}

const MARA: Palette = Palette {
    background: [0x3f, 0x5f, 0x7a],
    skin: [0xe0, 0xb2, 0x93],
    hair: [0xd9, 0xa4, 0x41],
    shirt: [0x1e, 0x29, 0x3b],
};
const PRIYA: Palette = Palette {
    background: [0x4b, 0x3b, 0x5c],
    skin: [0x9a, 0x6a, 0x4a],
    hair: [0x1c, 0x19, 0x17],
    shirt: [0xbe, 0x12, 0x3c],
};
const TOBIAS: Palette = Palette {
    background: [0x3b, 0x4a, 0x3f],
    skin: [0xf1, 0xc7, 0xa5],
    hair: [0x57, 0x53, 0x4e],
    shirt: [0x0f, 0x76, 0x6e],
};

pub const MARA_ID: &str = "demo-mara";
pub const JONAS_ID: &str = "demo-jonas";
pub const PRIYA_ID: &str = "demo-priya";
pub const LEA_ID: &str = "demo-lea";
pub const TOBIAS_ID: &str = "demo-tobias";

const DEMO_TEAMS: [(&str, &str, [&str; 3]); 4] = [
    (
        "demo-team-1",
        "Platform",
        ["General", "Incidents", "Releases"],
    ),
    ("demo-team-2", "Product", ["General", "Roadmap", "Feedback"]),
    (
        "demo-team-3",
        "Operations",
        ["General", "On call", "Runbooks"],
    ),
    (
        "demo-team-4",
        "Archive 2025",
        ["General", "Trade fair", "Office move"],
    ),
];
const DEMO_TEAM_ORDER: [&str; 4] = ["demo-team-2", "demo-team-1", "demo-team-3", "demo-team-4"];
const DEMO_HIDDEN_TEAM: &str = "demo-team-4";
const DEMO_HIDDEN_CHANNEL: &str = "demo-channel-3-3";

const UNREAD_CHAT: &str = "demo-chat-atlas";
const RELEASE_CHAT: &str = "demo-chat-release";
const PINNED_MESSAGE: &str = "m5b";
const SAVED_MESSAGES: [&str; 2] = ["m4", "m5c"];
const FORWARD_ITEMTYPE: &str = "http://schema.skype.com/Forward";
const FAVORITES_ID: &str = "demo-folder-favorites";
const CUSTOMERS_FOLDER: &str = "demo-folder-customers";
const ATLAS_FOLDER: &str = "demo-folder-atlas";
const TODO_FOLDER: &str = "demo-folder-todo";

fn member(user_id: &str, name: &str) -> MemberRecord {
    MemberRecord {
        user_id: Some(user_id.to_owned()),
        display_name: name.to_owned(),
    }
}

fn people(extra: &[(&str, &str)]) -> Vec<MemberRecord> {
    std::iter::once(member(DEMO_USER_ID, DEMO_USER_NAME))
        .chain(extra.iter().map(|(id, name)| member(id, name)))
        .collect()
}

pub fn at(days_ago: i64, hour: u32, minute: u32) -> DateTime<Utc> {
    let day = Local::now().date_naive() - Duration::days(days_ago);
    let local = day.and_hms_opt(hour, minute, 0).expect("valid time");
    Local
        .from_local_datetime(&local)
        .earliest()
        .map_or_else(Utc::now, |time| time.with_timezone(&Utc))
}

struct DemoChat {
    id: &'static str,
    kind: &'static str,
    title: &'static str,
    members: Vec<MemberRecord>,
    time: DateTime<Utc>,
    unread: bool,
    preview: Option<(&'static str, &'static str, &'static str)>,
    deleted: bool,
}

fn demo_chats() -> Vec<DemoChat> {
    let group = |extra: &[(&str, &str)]| people(extra);
    vec![
        DemoChat {
            id: BOT_CHAT,
            kind: "oneOnOne",
            title: BOT_NAME,
            members: people(&[]),
            time: at(0, 13, 45),
            unread: true,
            preview: Some(("", BOT_NAME, "Mara Lindqvist edited your page")),
            deleted: false,
        },
        DemoChat {
            id: "demo-chat-mara",
            kind: "oneOnOne",
            title: "Mara Lindqvist",
            members: people(&[(MARA_ID, "Mara Lindqvist")]),
            time: at(0, 13, 40),
            unread: false,
            preview: Some((MARA_ID, "Mara Lindqvist", "Sure, 2 pm works")),
            deleted: false,
        },
        DemoChat {
            id: RELEASE_CHAT,
            kind: "group",
            title: "Release planning",
            members: group(&[
                (MARA_ID, "Mara Lindqvist"),
                (JONAS_ID, "Jonas Ortega"),
                (PRIYA_ID, "Priya Nair"),
                (LEA_ID, "Lea Schneider"),
            ]),
            time: at(0, 13, 35),
            unread: false,
            preview: Some((
                DEMO_USER_ID,
                DEMO_USER_NAME,
                "Build 42 is green, see pipeline",
            )),
            deleted: false,
        },
        DemoChat {
            id: "demo-chat-customer-1",
            kind: "group",
            title: "Customer North - Operations",
            members: group(&[(LEA_ID, "Lea Schneider"), (TOBIAS_ID, "Tobias Klein")]),
            time: at(0, 12, 50),
            unread: true,
            preview: Some((LEA_ID, "Lea Schneider", "Access is enabled now")),
            deleted: false,
        },
        DemoChat {
            id: "demo-chat-customer-2",
            kind: "group",
            title: "Customer South - Rollout",
            members: group(&[(JONAS_ID, "Jonas Ortega"), (PRIYA_ID, "Priya Nair")]),
            time: at(0, 9, 5),
            unread: true,
            preview: Some((JONAS_ID, "Jonas Ortega", "Meeting is set for Friday")),
            deleted: false,
        },
        DemoChat {
            id: "demo-chat-customer-3",
            kind: "group",
            title: "Customer West - Support",
            members: group(&[(TOBIAS_ID, "Tobias Klein"), (LEA_ID, "Lea Schneider")]),
            time: at(1, 16, 0),
            unread: false,
            preview: Some((TOBIAS_ID, "Tobias Klein", "Ticket is closed")),
            deleted: false,
        },
        DemoChat {
            id: "demo-chat-customer-4",
            kind: "group",
            title: "Customer East - Pilot",
            members: group(&[(MARA_ID, "Mara Lindqvist"), (PRIYA_ID, "Priya Nair")]),
            time: at(3, 10, 0),
            unread: false,
            preview: Some((DEMO_USER_ID, DEMO_USER_NAME, "I'll send the documents")),
            deleted: false,
        },
        DemoChat {
            id: "demo-chat-atlas",
            kind: "group",
            title: "Atlas core team",
            members: group(&[
                (PRIYA_ID, "Priya Nair"),
                (LEA_ID, "Lea Schneider"),
                (MARA_ID, "Mara Lindqvist"),
            ]),
            time: at(0, 13, 12),
            unread: true,
            preview: Some((PRIYA_ID, "Priya Nair", "Who is taking the review?")),
            deleted: false,
        },
        DemoChat {
            id: "demo-chat-jonas",
            kind: "oneOnOne",
            title: "Jonas Ortega",
            members: people(&[(JONAS_ID, "Jonas Ortega")]),
            time: at(0, 11, 2),
            unread: false,
            preview: Some((DEMO_USER_ID, DEMO_USER_NAME, "Thanks, it's merged")),
            deleted: false,
        },
        DemoChat {
            id: "demo-chat-priya",
            kind: "oneOnOne",
            title: "Priya Nair",
            members: people(&[(PRIYA_ID, "Priya Nair")]),
            time: at(0, 10, 47),
            unread: false,
            preview: Some((PRIYA_ID, "Priya Nair", "Image")),
            deleted: false,
        },
        DemoChat {
            id: "demo-chat-tobias",
            kind: "oneOnOne",
            title: "Tobias Klein",
            members: people(&[(TOBIAS_ID, "Tobias Klein")]),
            time: at(2, 9, 30),
            unread: false,
            preview: Some((TOBIAS_ID, "Tobias Klein", "ok")),
            deleted: false,
        },
        DemoChat {
            id: "demo-chat-allhands",
            kind: "group",
            title: "All-hands announcements",
            members: group(&[(LEA_ID, "Lea Schneider"), (MARA_ID, "Mara Lindqvist")]),
            time: at(0, 9, 0),
            unread: true,
            preview: Some((LEA_ID, "Lea Schneider", "New vacation policy from November")),
            deleted: false,
        },
        DemoChat {
            id: "demo-chat-design",
            kind: "group",
            title: "Design review",
            members: group(&[(MARA_ID, "Mara Lindqvist"), (TOBIAS_ID, "Tobias Klein")]),
            time: at(2, 15, 0),
            unread: false,
            preview: Some((MARA_ID, "Mara Lindqvist", "alt")),
            deleted: true,
        },
        DemoChat {
            id: "demo-chat-quarterly",
            kind: "group",
            title: "Quarterly numbers, finance review and planning 2027",
            members: group(&[(LEA_ID, "Lea Schneider"), (JONAS_ID, "Jonas Ortega")]),
            time: at(4, 11, 0),
            unread: false,
            preview: Some((LEA_ID, "Lea Schneider", "Draft attached")),
            deleted: false,
        },
    ]
}

pub fn mention_candidates(conversation_id: &str, query: &str) -> Vec<MentionCandidate> {
    let needle = query.trim().to_lowercase();
    let matches = |name: &str| needle.is_empty() || name.to_lowercase().contains(&needle);
    let people = PEOPLE_DIRECTORY
        .iter()
        .filter(|(_, name, _, _)| matches(name))
        .map(|(user_id, name, job_title, mail)| {
            MentionCandidate::Person(PersonCandidate {
                user_id: (*user_id).to_owned(),
                display_name: (*name).to_owned(),
                mail: Some((*mail).to_owned()),
                job_title: Some((*job_title).to_owned()),
                source: PersonSource::Member,
            })
        });
    let scope: Vec<MentionCandidate> = if conversation_id.starts_with("demo-channel") {
        [
            MentionCandidate::Channel {
                channel_id: conversation_id.to_owned(),
                name: "General".to_owned(),
            },
            MentionCandidate::Team {
                team_id: "demo-team-1".to_owned(),
                name: "Platform".to_owned(),
            },
        ]
        .into_iter()
        .filter(|candidate| matches(candidate.display_name()))
        .collect()
    } else {
        Vec::new()
    };
    people.chain(scope).collect()
}

pub fn send_in_chat(
    chat: &ChatRecord,
    preview: &str,
    html: &str,
    now: DateTime<Utc>,
) -> (ChatRecord, MessageRecord) {
    let mut chat = chat.clone();
    chat.last_message_at = Some(now);
    chat.last_message_preview = Some(preview.to_owned());
    chat.last_message_sender_id = Some(DEMO_USER_ID.to_owned());
    chat.last_message_sender_name = Some(DEMO_USER_NAME.to_owned());
    chat.last_message_deleted = false;
    chat.unread = false;
    let record = message(
        &chat.id,
        &format!("demo-sent-{}", now.timestamp_millis()),
        None,
        (DEMO_USER_ID, DEMO_USER_NAME),
        now,
        html,
        "[]",
        false,
    );
    (chat, record)
}

pub fn new_chat(
    existing: &[ChatRecord],
    people: &[(String, String)],
    topic: Option<&str>,
    preview: &str,
    html: &str,
    now: DateTime<Utc>,
) -> (ChatRecord, MessageRecord) {
    let number = existing
        .iter()
        .filter(|chat| chat.id.starts_with(NEW_CHAT_PREFIX))
        .count()
        + 1;
    let one_on_one = people.len() == 1;
    let members = std::iter::once(member(DEMO_USER_ID, DEMO_USER_NAME))
        .chain(people.iter().map(|(user_id, name)| member(user_id, name)))
        .collect();
    let chat = ChatRecord {
        id: format!("{NEW_CHAT_PREFIX}{number}"),
        kind: if one_on_one { "oneOnOne" } else { "group" }.to_owned(),
        title: if one_on_one {
            String::new()
        } else {
            topic.unwrap_or_default().to_owned()
        },
        member_summary: people
            .iter()
            .map(|(_, name)| name.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        members,
        ..Default::default()
    };
    send_in_chat(&chat, preview, html, now)
}

fn image_directory() -> PathBuf {
    std::env::temp_dir().join("rusty-teams-demo")
}

pub fn search_gifs(query: &str) -> Vec<Gif> {
    let query = query.trim().to_lowercase();
    SEARCHABLE_GIFS
        .get()
        .into_iter()
        .flatten()
        .filter(|gif| gif.title.to_lowercase().contains(&query))
        .cloned()
        .collect()
}

pub fn first_unread(conversation_id: &str) -> Option<String> {
    (conversation_id == UNREAD_CHAT).then(|| "u3".to_owned())
}

pub fn read_message_ids(records: &[MessageRecord]) -> std::collections::HashSet<String> {
    let newest = records.iter().max_by_key(|record| record.created_at);
    records
        .iter()
        .filter(|record| {
            Some(record.message_id.as_str()) != newest.map(|newest| newest.message_id.as_str())
        })
        .map(|record| record.message_id.clone())
        .collect()
}

pub fn first_selection() -> Selection {
    Selection::Chat(RELEASE_CHAT.to_owned())
}

pub fn activity_entries() -> Vec<Entry> {
    let actor = |user_id: &str, name: &str| Actor {
        user_id: Some(user_id.to_owned()),
        name: name.to_owned(),
    };
    let entry = |conversation_id: &str,
                 kind: Kind,
                 message_id: &str,
                 actors: Vec<Actor>,
                 preview: &str,
                 count: u32,
                 updated_at: DateTime<Utc>,
                 read: bool| Entry {
        id: 0,
        conversation_id: conversation_id.to_owned(),
        kind,
        message_id: message_id.to_owned(),
        actors,
        preview: preview.to_owned(),
        glyphs: Vec::new(),
        count,
        updated_at,
        read,
    };
    let reaction = Entry {
        glyphs: vec!["\u{1F602}".to_owned(), "\u{1F44D}".to_owned()],
        ..entry(
            RELEASE_CHAT,
            Kind::Reaction,
            "m3",
            vec![
                actor(PRIYA_ID, "Priya Nair"),
                actor(LEA_ID, "Lea Schneider"),
            ],
            "I'll take the changelog.",
            2,
            at(0, 13, 20),
            false,
        )
    };
    vec![
        entry(
            UNREAD_CHAT,
            Kind::Messages,
            "u3",
            vec![
                actor(LEA_ID, "Lea Schneider"),
                actor(PRIYA_ID, "Priya Nair"),
            ],
            "Who is taking the review?",
            3,
            at(0, 13, 12),
            false,
        ),
        entry(
            "demo-channel-1-1",
            Kind::Mention,
            "t1",
            vec![actor(MARA_ID, "Mara Lindqvist")],
            "@Dana can you approve the merge? The pipeline is waiting.",
            1,
            at(0, 11, 40),
            false,
        ),
        reaction,
        entry(
            "demo-chat-customer-3",
            Kind::Messages,
            "demo-customer-3-closed",
            vec![actor(TOBIAS_ID, "Tobias Klein")],
            "Ticket is closed",
            1,
            at(1, 16, 0),
            true,
        ),
    ]
}

pub fn seed(store: &Store) {
    let chats: Vec<ChatRecord> = demo_chats()
        .into_iter()
        .map(|demo| ChatRecord {
            id: demo.id.to_owned(),
            kind: demo.kind.to_owned(),
            title: if demo.kind == "oneOnOne" {
                String::new()
            } else {
                demo.title.to_owned()
            },
            member_summary: demo.title.to_owned(),
            last_message_at: Some(demo.time),
            unread: demo.unread,
            members: demo.members,
            last_message_preview: demo.preview.map(|(_, _, text)| text.to_owned()),
            last_message_sender_id: demo.preview.map(|(id, _, _)| id.to_owned()),
            last_message_sender_name: demo.preview.map(|(_, name, _)| name.to_owned()),
            last_message_deleted: demo.deleted,
            ..Default::default()
        })
        .collect();
    let _ = store.upsert_chats(&chats);
    let teams: Vec<TeamRecord> = DEMO_TEAMS
        .iter()
        .map(|(id, name, _)| TeamRecord {
            id: (*id).to_owned(),
            name: (*name).to_owned(),
        })
        .collect();
    let _ = store.upsert_teams(&teams);
    let mut channels = Vec::new();
    for (team_index, (team_id, _, names)) in DEMO_TEAMS.iter().enumerate() {
        for (channel_index, name) in names.iter().enumerate() {
            channels.push(ChannelRecord {
                id: format!("demo-channel-{}-{}", team_index + 1, channel_index + 1),
                team_id: (*team_id).to_owned(),
                name: (*name).to_owned(),
                membership_type: None,
                last_message_at: Some(at(channel_index as i64, 12, 10)),
                unread: (team_index == 0 && channel_index == 1)
                    || (team_index == 1 && channel_index == 0),
            });
        }
    }
    let _ = store.upsert_channels(&channels);
    let _ = store.replace_team_layout(&demo_team_layout(&channels));
    let _ = store.set_meta("me_user_id", DEMO_USER_ID);
    let _ = store.set_meta("me_display_name", DEMO_USER_NAME);
    let _ = store.upsert_messages(&release_messages());
    let _ = store.upsert_messages(&unread_messages());
    let _ = store.upsert_messages(&channel_messages());
    let _ = store.upsert_messages(&priority_messages());
    let _ = store.upsert_messages(&bot_messages());
}

fn photo(palette: &Palette) -> Arc<Image> {
    crate::data::avatar_image_from("image/png", encode_png(palette))
}

pub fn seed_directory(state: &mut AppState) {
    state.own_status = crate::own_status::demo_status();
    state.own_email = DEMO_USER_EMAIL.to_owned();
    let directory = &mut state.directory;
    directory.me = Some(Person {
        user_id: DEMO_USER_ID.to_owned(),
        display_name: DEMO_USER_NAME.to_owned(),
    });
    let ids = |list: &[&str]| list.iter().map(|id| (*id).to_owned()).collect::<Vec<_>>();
    directory.folders = vec![
        FolderInfo {
            id: FAVORITES_ID.to_owned(),
            name: String::new(),
            kind: FolderKind::Favorites,
            conversation_ids: ids(&["demo-chat-mara", RELEASE_CHAT]),
        },
        FolderInfo {
            id: CUSTOMERS_FOLDER.to_owned(),
            name: "Customers".to_owned(),
            kind: FolderKind::UserCreated,
            conversation_ids: ids(&[
                "demo-chat-customer-1",
                "demo-chat-customer-2",
                "demo-chat-customer-3",
                "demo-chat-customer-4",
            ]),
        },
        FolderInfo {
            id: ATLAS_FOLDER.to_owned(),
            name: "Project Atlas".to_owned(),
            kind: FolderKind::UserCreated,
            conversation_ids: ids(&["demo-chat-atlas", "demo-chat-jonas"]),
        },
        FolderInfo {
            id: TODO_FOLDER.to_owned(),
            name: "To do".to_owned(),
            kind: FolderKind::UserCreated,
            conversation_ids: Vec::new(),
        },
    ];
    state.bot_apps.insert(BOT_CHAT.to_owned(), vec![bot_app()]);
    let directory = &mut state.directory;
    directory.pinned_channels = ids(&["demo-channel-1-1", "demo-channel-3-2"]);
    directory
        .unread_counts
        .insert("demo-chat-atlas".to_owned(), 5);
    directory
        .unread_counts
        .insert("demo-chat-customer-1".to_owned(), 2);
    directory
        .unread_counts
        .insert("demo-chat-customer-2".to_owned(), 1);
    directory
        .unread_counts
        .insert("demo-chat-allhands".to_owned(), 120);
    directory.set_avatar(MARA_ID, Some(photo(&MARA)));
    directory.set_avatar(PRIYA_ID, Some(photo(&PRIYA)));
    directory.set_avatar(TOBIAS_ID, Some(photo(&TOBIAS)));
    for (user_id, kind) in [
        (MARA_ID, PresenceKind::Available),
        (JONAS_ID, PresenceKind::DoNotDisturb),
        (PRIYA_ID, PresenceKind::Away),
        (TOBIAS_ID, PresenceKind::Offline),
        (LEA_ID, PresenceKind::DoNotDisturb),
        (DEMO_USER_ID, PresenceKind::Available),
    ] {
        directory.set_presence(user_id, kind);
    }
    let directory_path = image_directory();
    if std::fs::create_dir_all(&directory_path).is_ok() {
        let path = directory_path.join("staging-dashboard.png");
        let png = encode_rgb(STAGING_SIZE.0, STAGING_SIZE.1, staging_pixel);
        if std::fs::write(&path, png).is_ok() {
            state.directory.set_image(STAGING_IMAGE_KEY, &path);
            state.directory.set_image(LINK_IMAGE_KEY, &path);
        }
        let _ = SEARCHABLE_GIFS.set(demo_gifs::generate(&directory_path));
        state.directory.set_image(
            demo_gifs::RECEIVED_GIF_KEY,
            &directory_path.join(demo_gifs::file_name(demo_gifs::RECEIVED_GIF_NAME)),
        );
    }
    state.typing.start(
        "demo-chat-mara",
        MARA_ID,
        "Mara Lindqvist",
        Instant::now(),
        Utc::now(),
    );
    state.collapsed.insert(CUSTOMERS_FOLDER.to_owned());
    state.last_sync = Some(Utc::now());
    state.apply_bot_titles();
    seed_message_actions(state);
}

fn seed_message_actions(state: &mut AppState) {
    state.pins.insert(
        RELEASE_CHAT.to_owned(),
        vec![PinnedMessage {
            message_id: PINNED_MESSAGE.to_owned(),
            pinned_at: Some(at(0, 11, 30)),
            parent_id: None,
        }],
    );
    let ids: Vec<String> = SAVED_MESSAGES.iter().map(|id| (*id).to_owned()).collect();
    let records = state
        .store
        .messages_by_id(RELEASE_CHAT, &ids)
        .unwrap_or_default();
    let saved = SAVED_MESSAGES
        .iter()
        .enumerate()
        .filter_map(|(position, id)| {
            let record = records.get(*id)?;
            Some(SavedMessage {
                conversation_id: record.conversation_id.clone(),
                message_id: record.message_id.clone(),
                root_id: record.message_id.clone(),
                author_id: record.sender_id.clone(),
                author_name: record.sender_name.clone(),
                preview: crate::rows::reply_excerpt(record),
                saved_at: at(position as i64, 15, 20),
                topic: None,
            })
        })
        .collect();
    state.saved.replace(saved);
}

pub fn forwarded_message(
    chat: &ChatRecord,
    source: &ForwardSource,
    comment: &str,
    now: DateTime<Utc>,
) -> (ChatRecord, MessageRecord) {
    let comment_html = if comment.is_empty() {
        String::new()
    } else {
        format!("<p>{}</p>", teams_core::escape_html(comment))
    };
    let html = format!(
        "{comment_html}<blockquote itemscope=\"\" itemtype=\"{FORWARD_ITEMTYPE}\" itemid=\"{}\"><strong>{}</strong><p>{}</p></blockquote>",
        teams_core::escape_html(&source.message_id),
        teams_core::escape_html(&source.author),
        teams_core::escape_html(&source.text),
    );
    send_in_chat(chat, &source.text, &html, now)
}

#[allow(clippy::too_many_arguments)]
fn message(
    conversation: &str,
    id: &str,
    reply_to: Option<&str>,
    sender: (&str, &str),
    time: DateTime<Utc>,
    html: &str,
    reactions_json: &str,
    edited: bool,
) -> MessageRecord {
    MessageRecord {
        conversation_id: conversation.to_owned(),
        message_id: id.to_owned(),
        reply_to_id: reply_to.map(str::to_owned),
        sender_id: Some(sender.0.to_owned()),
        sender_name: Some(sender.1.to_owned()),
        sender_application_id: None,
        links_json: "[]".to_owned(),
        created_at: time,
        edited_at: edited.then_some(time + Duration::minutes(2)),
        deleted: false,
        body_html: html.to_owned(),
        attachments_json: "[]".to_owned(),
        reactions_json: reactions_json.to_owned(),
        mentions_json: "[]".to_owned(),
        subject: None,
    }
}

fn with_subject(mut record: MessageRecord, subject: &str) -> MessageRecord {
    record.subject = Some(subject.to_owned());
    record
}

fn with_attachments(mut record: MessageRecord, attachments_json: &str) -> MessageRecord {
    record.attachments_json = attachments_json.to_owned();
    record
}

fn with_links(mut record: MessageRecord, links_json: String) -> MessageRecord {
    record.links_json = links_json;
    record
}

pub fn link_preview(url: &str) -> Option<LinkPreview> {
    Some(LinkPreview {
        url: url.to_owned(),
        title: Some("Release checklist Q4".to_owned()),
        description: Some(
            "Everything that has to be green before the release candidate goes out on Friday."
                .to_owned(),
        ),
        image_url: Some(LINK_IMAGE_KEY.to_owned()),
        image_width: Some(LINK_IMAGE_SIZE.0),
        image_height: Some(LINK_IMAGE_SIZE.1),
    })
}

fn bot_messages() -> Vec<MessageRecord> {
    let card = serde_json::json!({
        "type": "AdaptiveCard",
        "body": [
            {"type": "ColumnSet", "columns": [
                {"type": "Column", "width": "auto", "verticalContentAlignment": "center", "items": [
                    {"type": "Image", "url": CARD_PERSON_URL, "size": "small", "style": "person", "height": "32px"}
                ]},
                {"type": "Column", "width": "stretch", "verticalContentAlignment": "center", "items": [
                    {"type": "TextBlock", "size": "medium", "weight": "bolder", "wrap": true, "text": "Mara Lindqvist edited your page"}
                ]}
            ]},
            {"type": "ColumnSet", "separator": true, "columns": [
                {"type": "Column", "width": "auto", "items": [
                    {"type": "Image", "url": CARD_PAGE_URL, "width": "24px", "height": "24px"}
                ]},
                {"type": "Column", "width": "stretch", "items": [
                    {"type": "TextBlock", "weight": "bolder", "wrap": true, "text": "[**Release Checklist Q4**](https://example.com/wiki/release-checklist) in [**Platform**](https://example.com/wiki/platform)"},
                    {"type": "TextBlock", "spacing": "small", "isSubtle": true, "wrap": true, "text": "Owned by: Dana Demo"}
                ]}
            ]},
            {"type": "TextBlock", "id": "owner-note", "isVisible": false, "isSubtle": true, "wrap": true, "text": "Owner notes are only shown on request."},
            {"type": "ActionSet", "actions": [
                {"type": "Action.OpenUrl", "title": "View page", "url": "https://example.com/wiki/release-checklist"},
                {"type": "Action.OpenUrl", "title": "View changes", "url": "https://example.com/wiki/release-checklist/changes"},
                {"type": "Action.Submit", "title": "Watch page", "data": {"action": "watch"}},
                {"type": "Action.Submit", "title": "Stop watching", "data": {"action": "unwatch"}},
                {"type": "Action.Submit", "title": "Settings", "data": {"msteams": {"type": "task/fetch"}}},
                {"type": "Action.ToggleVisibility", "title": "Owner notes", "targetElements": ["owner-note"]},
                {"type": "Action.ShowCard", "title": "Details", "card": {"body": [
                    {"type": "TextBlock", "wrap": true, "text": "Last edited by **Mara Lindqvist** on Tuesday."}
                ]}}
            ]}
        ]
    });
    let mut messages = vec![
        bot_card_message("b1", at(0, 13, 45), &card),
        bot_card_message("b2", at(0, 13, 52), &input_card()),
        bot_card_message("b3", at(0, 13, 58), &crate::demo_input_cards::all_inputs_card()),
    ];
    messages.extend(
        crate::demo_cards::behaviour_cards()
            .iter()
            .zip(0u32..)
            .map(|(card, index)| bot_card_message(&format!("g{index}"), at(0, 14, index), card)),
    );
    messages.extend(
        crate::demo_chart_cards::cards()
            .iter()
            .zip(0..)
            .map(|(chart_card, minute)| {
                bot_card_message(&format!("c{minute}"), at(0, 15, minute), chart_card)
            }),
    );
    messages
}

fn bot_card_message(id: &str, time: DateTime<Utc>, card: &serde_json::Value) -> MessageRecord {
    let attachments = serde_json::json!([{
        "content_type": "application/vnd.microsoft.card.adaptive",
        "name": null,
        "url": null,
        "text": null,
        "content": card.to_string()
    }]);
    let mut record = with_attachments(
        message(
            BOT_CHAT,
            id,
            None,
            ("", BOT_NAME),
            time,
            &format!("<attachment id=\"{id}\"></attachment>"),
            "[]",
            false,
        ),
        &attachments.to_string(),
    );
    record.sender_id = None;
    record.sender_application_id = Some(BOT_ID.to_owned());
    record
}

fn input_card() -> serde_json::Value {
    serde_json::json!({
        "type": "AdaptiveCard",
        "body": [
            {"type": "TextBlock", "size": "medium", "weight": "bolder", "text": "Release request"},
            {"type": "Input.Text", "id": "title", "label": "Title", "placeholder": "Short summary", "isRequired": true, "errorMessage": "Give the release a title"},
            {"type": "Input.Text", "id": "notes", "label": "Notes", "placeholder": "Anything the reviewers should know", "isMultiline": true},
            {"type": "Input.Text", "id": "token", "label": "Access code", "style": "Password", "regex": "^\\d{4}$", "errorMessage": "Enter four digits"},
            {"type": "ColumnSet", "columns": [
                {"type": "Column", "width": "stretch", "items": [
                    {"type": "Input.Number", "id": "count", "label": "Servers", "placeholder": "1-20", "min": 1, "max": 20, "value": 4}
                ]},
                {"type": "Column", "width": "stretch", "items": [
                    {"type": "Input.Date", "id": "day", "label": "Release day", "value": "2026-11-02"}
                ]},
                {"type": "Column", "width": "stretch", "items": [
                    {"type": "Input.Time", "id": "hour", "label": "Start time", "value": "09:30"}
                ]}
            ]},
            {"type": "Input.ChoiceSet", "id": "channel", "label": "Channel", "placeholder": "Pick a channel", "isRequired": true, "choices": [
                {"title": "Stable", "value": "stable"}, {"title": "Beta", "value": "beta"}, {"title": "Nightly", "value": "nightly"}
            ]},
            {"type": "Input.ChoiceSet", "id": "region", "label": "Region", "style": "expanded", "value": "eu", "choices": [
                {"title": "Europe", "value": "eu"}, {"title": "North America", "value": "na"}
            ]},
            {"type": "Input.ChoiceSet", "id": "checks", "label": "Checks", "style": "expanded", "isMultiSelect": true, "value": "lint", "choices": [
                {"title": "Lint", "value": "lint"}, {"title": "Unit tests", "value": "unit"}, {"title": "Smoke tests", "value": "smoke"}
            ]},
            {"type": "Input.Toggle", "id": "notify", "title": "Notify the team", "value": "yes", "valueOn": "yes", "valueOff": "no"}
        ],
        "actions": [
            {"type": "Action.Submit", "title": "Request release", "data": {"action": "release"}},
            {"type": "Action.Submit", "title": "Cancel", "associatedInputs": "none", "data": {"action": "cancel"}}
        ]
    })
}

fn bot_app() -> ChatApp {
    ChatApp {
        app_id: "demo-app-wiki".to_owned(),
        name: BOT_APP_NAME.to_owned(),
        bot_ids: vec![BOT_ID.to_owned()],
        small_image_url: None,
        accent_color: None,
        web_application_resource: None,
    }
}

pub async fn card_answer(task: CardTask) -> CardAnswer {
    tokio::time::sleep(CARD_ANSWER_DELAY).await;
    let data = match task {
        CardTask::Submit(_) => {
            return Ok((
                CardActionOutcome::Message("Settings saved".into()),
                Some((bot_app(), BOT_ID.to_owned())),
            ));
        }
        CardTask::Action(action) => match action.invoke_payload() {
            Some(payload) => payload.value,
            None => return Err("Not supported".to_owned()),
        },
    };
    let outcome = if data.pointer("/data/type").and_then(|kind| kind.as_str()) == Some("task/fetch")
    {
        CardActionOutcome::Dialog(TaskDialog {
            title: None,
            width: 520,
            height: 300,
            kind: TaskDialogKind::Card(settings_card().to_string()),
        })
    } else if data.get("action").and_then(|action| action.as_str()) == Some("unwatch") {
        CardActionOutcome::Failed("The app answered 500: demo failure".into())
    } else {
        CardActionOutcome::Sent
    };
    Ok((outcome, Some((bot_app(), BOT_ID.to_owned()))))
}

pub async fn refresh_answer() -> CardAnswer {
    tokio::time::sleep(CARD_ANSWER_DELAY).await;
    Ok((
        CardActionOutcome::ReplaceCard(crate::demo_cards::refreshed_card().to_string()),
        Some((bot_app(), BOT_ID.to_owned())),
    ))
}

fn settings_card() -> serde_json::Value {
    serde_json::json!({
        "type": "AdaptiveCard",
        "body": [
            {"type": "TextBlock", "weight": "bolder", "text": "Notification settings"},
            {"type": "TextBlock", "wrap": true, "isSubtle": true, "text": "Choose how the bot tells you about page changes."},
            {"type": "Input.ChoiceSet", "id": "frequency", "label": "Frequency", "style": "expanded", "value": "daily", "choices": [
                {"title": "Immediately", "value": "now"}, {"title": "Daily digest", "value": "daily"}
            ]},
            {"type": "Input.Toggle", "id": "mentions", "title": "Only when I am mentioned", "value": "false"}
        ],
        "actions": [
            {"type": "Action.Submit", "title": "Save", "data": {"save": true}}
        ]
    })
}

fn priority_messages() -> Vec<MessageRecord> {
    let priya = (PRIYA_ID, "Priya Nair");
    let me = (DEMO_USER_ID, DEMO_USER_NAME);
    vec![
        message(
            PRIORITY_CHAT,
            "p1",
            None,
            me,
            at(0, 10, 40),
            "<p>Have you seen the error on staging yet?</p>",
            "[]",
            false,
        ),
        message(
            PRIORITY_CHAT,
            "p2",
            None,
            priya,
            at(0, 10, 47),
            &format!(
                "<p>Yes, here is the screenshot:</p><img src=\"{PENDING_IMAGE_KEY}\" width=\"1280\" height=\"720\">"
            ),
            "[]",
            false,
        ),
    ]
}

fn reaction(kind: &str, users: &[(&str, &str, DateTime<Utc>)]) -> Vec<serde_json::Value> {
    users
        .iter()
        .map(|(user_id, user_name, created_at)| {
            serde_json::json!({
                "reaction_type": kind,
                "user_id": user_id,
                "user_name": user_name,
                "created_at": created_at,
            })
        })
        .collect()
}

fn reactions_json(groups: Vec<Vec<serde_json::Value>>) -> String {
    serde_json::Value::Array(groups.into_iter().flatten().collect()).to_string()
}

fn release_messages() -> Vec<MessageRecord> {
    let chat = RELEASE_CHAT;
    let mara = (MARA_ID, "Mara Lindqvist");
    let jonas = (JONAS_ID, "Jonas Ortega");
    let me = (DEMO_USER_ID, DEMO_USER_NAME);
    let jonas_name = "Jonas Ortega";
    let priya_name = "Priya Nair";
    let lea_name = "Lea Schneider";
    let mara_name = "Mara Lindqvist";
    let tobias_name = "Tobias Klein";
    let likes = reactions_json(vec![reaction(
        "like",
        &[
            (JONAS_ID, jonas_name, at(1, 16, 30)),
            (PRIYA_ID, priya_name, at(1, 17, 2)),
            (LEA_ID, lea_name, at(0, 8, 15)),
        ],
    )]);
    let hearts = reactions_json(vec![reaction(
        "heart",
        &[
            (DEMO_USER_ID, DEMO_USER_NAME, at(0, 10, 35)),
            (JONAS_ID, jonas_name, at(0, 10, 33)),
        ],
    )]);
    let single = reactions_json(vec![reaction(
        "laugh",
        &[(PRIYA_ID, priya_name, at(0, 9, 10))],
    )]);
    let many = reactions_json(vec![
        reaction(
            "like",
            &[
                (MARA_ID, mara_name, at(0, 13, 37)),
                (JONAS_ID, jonas_name, at(0, 13, 38)),
                (PRIYA_ID, priya_name, at(0, 13, 40)),
                (LEA_ID, lea_name, at(0, 13, 45)),
                (TOBIAS_ID, tobias_name, at(0, 13, 50)),
            ],
        ),
        reaction(
            "heart",
            &[
                (LEA_ID, lea_name, at(0, 13, 46)),
                (TOBIAS_ID, tobias_name, at(0, 13, 51)),
            ],
        ),
        reaction("\u{1F389}", &[(MARA_ID, mara_name, at(0, 13, 39))]),
    ]);
    vec![
        message(
            chat,
            "m1",
            None,
            mara,
            at(1, 16, 20),
            "<p>Release candidate is on staging. Can someone run the regression suite?</p>",
            "[]",
            false,
        ),
        message(
            chat,
            "m2",
            None,
            mara,
            at(1, 16, 22),
            "<p>Checklist lives in <a href=\"https://example.com/checklist\">release checklist</a>.</p>",
            &likes,
            false,
        ),
        with_links(
            message(
                chat,
                "m2a",
                None,
                mara,
                at(1, 16, 23),
                "<p><a href=\"https://example.com/checklist\">https://example.com/checklist</a></p>",
                "[]",
                false,
            ),
            link_preview("https://example.com/checklist")
                .map(|preview| preview.links_json())
                .unwrap_or_default(),
        ),
        message(
            chat,
            "m2b",
            None,
            mara,
            at(1, 16, 40),
            &format!(
                "<p>Staging dashboard right now:</p><img src=\"{STAGING_IMAGE_KEY}\" width=\"{}\" height=\"{}\">",
                STAGING_SIZE.0, STAGING_SIZE.1
            ),
            "[]",
            false,
        ),
        with_attachments(
            message(
                chat,
                "m2c",
                None,
                mara,
                at(1, 16, 41),
                "<p>Checklist and rollout plan attached.</p>",
                "[]",
                false,
            ),
            r#"[{"content_type":"reference","name":"Release-Checklist-Q4.xlsx","url":"https://example.com/files/checklist.xlsx","text":null,"size":48213},{"content_type":"reference","name":"Rollout-Plan.pdf","url":"https://example.com/files/plan.pdf","text":null,"size":2306867}]"#,
        ),
        message(
            chat,
            "m3",
            None,
            me,
            at(0, 9, 2),
            "<p>I'll take the changelog.</p>",
            &single,
            false,
        ),
        message(
            chat,
            "m4",
            None,
            jonas,
            at(0, 10, 14),
            "<p><at id=\"0\">Mara Lindqvist</at> can you take a quick look at this? It runs before <code>cargo test</code>.</p><pre><code class=\"language-rust\">// only release branches\nfn freeze(branch: &amp;str) -&gt; bool {\n    branch.starts_with(\"release/\")\n}</code></pre>",
            "[]",
            false,
        ),
        message(
            chat,
            "m5",
            None,
            mara,
            at(0, 10, 31),
            "<blockquote itemscope=\"\" itemtype=\"http://schema.skype.com/Reply\" itemid=\"m4\"><strong itemprop=\"mri\">Jonas Ortega</strong><p itemprop=\"preview\">can you take a quick look at this?</p></blockquote><p>Looks good. Small thing: the name could say what it checks.</p>",
            &hearts,
            true,
        ),
        message(
            chat,
            "m5b",
            None,
            mara,
            at(0, 10, 40),
            "<p>Rollout for Friday:</p><ul><li>Freeze the release branch</li><li>Run the smoke tests on staging, then post the results in the channel</li><li>Ship<ul><li>Windows installer</li><li>Linux <code>.AppImage</code></li></ul></li></ul><p>Order:</p><ol><li>Tag</li><li>Build</li><li>Announce</li></ol>",
            "[]",
            false,
        ),
        message(
            chat,
            "m5c",
            None,
            jonas,
            at(0, 11, 5),
            "<h2>Release notes 0.2</h2><p>The <s>old sync path</s> <u>is gone</u>.</p><table><thead><tr><th>Area</th><th>Owner</th></tr></thead><tbody><tr><td>Sync</td><td>Priya</td></tr><tr><td>Render</td><td>Jonas</td></tr></tbody></table><hr><blockquote>Measure twice, cut once.</blockquote><p><span style=\"background-color: rgb(255, 255, 0)\">Important:</span> <span style=\"color: #c00000\">backup first</span>.</p>",
            "[]",
            false,
        ),
        message(
            chat,
            "m5d",
            None,
            (PRIYA_ID, priya_name),
            at(0, 12, 20),
            &format!(
                "<p>FYI, the customer asked for this.</p><blockquote itemscope=\"\" itemtype=\"{FORWARD_ITEMTYPE}\" itemid=\"1790000000000\"><strong>Tobias Klein</strong><p>Can we move the freeze to Thursday? The customer demo is on Friday morning.</p></blockquote>"
            ),
            "[]",
            false,
        ),
        message(
            chat,
            "m6",
            None,
            me,
            at(0, 13, 35),
            "<p>Thanks, renamed. Tagging now.</p>",
            "[]",
            false,
        ),
        message(
            chat,
            "m7",
            None,
            me,
            at(0, 13, 36),
            "<p>Build 42 is green, see <a href=\"https://example.com/build/42\">pipeline</a>.</p>",
            &many,
            false,
        ),
        with_links(
            message(
                chat,
                "m7a",
                None,
                me,
                at(0, 13, 37),
                "<p>Notes are in <a href=\"https://example.com/notes\">https://example.com/notes</a></p>",
                "[]",
                false,
            ),
            LinkPreview {
                url: "https://example.com/notes".to_owned(),
                title: Some("Release notes 0.2".to_owned()),
                description: None,
                image_url: None,
                image_width: None,
                image_height: None,
            }
            .links_json(),
        ),
        message(
            chat,
            "m8",
            None,
            jonas,
            at(0, 13, 40),
            "<p>Formula check: H<sub>2</sub>O and x<sup>2</sup> + y<sup>2</sup>. <span style=\"font-size:xx-small;\">Small print, reviewed by <at id=\"0\">Mara Lindqvist</at>.</span> <span style=\"font-size:x-large;\">Big news:</span> the <span style=\"font-size:x-large;\"><a href=\"https://example.com/notes\">release notes</a></span> are out.</p>",
            "[]",
            false,
        ),
        message(
            chat,
            "m9",
            None,
            (PRIYA_ID, priya_name),
            at(0, 13, 45),
            &format!(
                "<p>Ship it!</p><img src=\"{}\" width=\"{}\" height=\"{}\" alt=\"Party\" itemtype=\"http://schema.skype.com/Giphy\">",
                demo_gifs::RECEIVED_GIF_KEY,
                demo_gifs::RECEIVED_GIF_SIZE.0,
                demo_gifs::RECEIVED_GIF_SIZE.1
            ),
            "[]",
            false,
        ),
    ]
}

fn unread_messages() -> Vec<MessageRecord> {
    let me = (DEMO_USER_ID, DEMO_USER_NAME);
    let lea = (LEA_ID, "Lea Schneider");
    let tobias = (TOBIAS_ID, "Tobias Klein");
    let rows = [
        (
            "u1",
            me,
            at(1, 17, 5),
            "<p>Can you enable access for North?</p>",
        ),
        ("u2", tobias, at(1, 17, 20), "<p>I'll check with IT.</p>"),
        (
            "u3",
            lea,
            at(0, 12, 40),
            "<p>Good morning, access is ready.</p>",
        ),
        (
            "u4",
            lea,
            at(0, 12, 41),
            "<p>Sign in with your company ID, no password needed.</p>",
        ),
        ("u5", lea, at(0, 12, 50), "<p>Access is enabled now</p>"),
    ];
    rows.into_iter()
        .map(|(id, sender, time, html)| {
            message(UNREAD_CHAT, id, None, sender, time, html, "[]", false)
        })
        .collect()
}

fn demo_team_layout(channels: &[ChannelRecord]) -> Vec<TeamLayoutRecord> {
    DEMO_TEAM_ORDER
        .iter()
        .map(|team_id| TeamLayoutRecord {
            team_id: (*team_id).to_owned(),
            hidden: *team_id == DEMO_HIDDEN_TEAM,
            channels: channels
                .iter()
                .filter(|channel| channel.team_id == *team_id)
                .map(|channel| ChannelLayoutRecord {
                    channel_id: channel.id.clone(),
                    general: channel.name == "General",
                    hidden: channel.id == DEMO_HIDDEN_CHANNEL,
                })
                .collect(),
        })
        .collect()
}

const ROLLOUT_REPLIES: [(&str, &str); 12] = [
    (MARA_ID, "Staging looks good on my side."),
    (JONAS_ID, "Smoke tests passed, no regressions."),
    (PRIYA_ID, "Docs for the new settings page are merged."),
    (LEA_ID, "Support is briefed, nothing blocking."),
    (MARA_ID, "Can we keep the old export for one more release?"),
    (JONAS_ID, "Yes, behind a flag. I will add it today."),
    (PRIYA_ID, "Changelog draft is ready for review."),
    (LEA_ID, "Customer success wants a heads-up the day before."),
    (MARA_ID, "I will send that mail on Thursday."),
    (JONAS_ID, "Flag is in, rebuilding staging now."),
    (PRIYA_ID, "Rollback steps are in the runbook."),
    (LEA_ID, "All green from QA. Go for Friday."),
];

fn channel_messages() -> Vec<MessageRecord> {
    let channel = "demo-channel-1-1";
    let mara = (MARA_ID, "Mara Lindqvist");
    let tobias = (TOBIAS_ID, "Tobias Klein");
    let people = [
        (MARA_ID, "Mara Lindqvist"),
        (JONAS_ID, "Jonas Ortega"),
        (PRIYA_ID, "Priya Nair"),
        (LEA_ID, "Lea Schneider"),
    ];
    let rollout_replies = ROLLOUT_REPLIES
        .iter()
        .enumerate()
        .map(|(index, (sender, text))| {
            let sender = people
                .iter()
                .copied()
                .find(|(id, _)| id == sender)
                .expect("known person");
            message(
                channel,
                &format!("t3r{}", index + 1),
                Some("t3"),
                sender,
                at(0, 9, 40) + Duration::minutes(6 * index as i64),
                &format!("<p>{text}</p>"),
                "[]",
                false,
            )
        });
    let mut records = vec![
        message(
            channel,
            "t1",
            None,
            mara,
            at(0, 8, 0),
            "<p>Weekly sync notes are up.</p>",
            "[]",
            false,
        ),
        message(
            channel,
            "t1r1",
            Some("t1"),
            tobias,
            at(0, 8, 20),
            "<p>Thanks, adding my items.</p>",
            "[]",
            false,
        ),
        with_subject(
            message(
                channel,
                "t3",
                None,
                tobias,
                at(0, 9, 30),
                "<p>Release build 42 is green, deploying to staging now.</p>",
                "[]",
                false,
            ),
            "Release 42 rollout",
        ),
        message(
            channel,
            "t2",
            None,
            tobias,
            at(0, 10, 0),
            "<p>Who owns the on-call handover?</p>",
            "[]",
            false,
        ),
        message(
            channel,
            "t4",
            None,
            mara,
            at(3, 8, 42),
            "<p>Sprint Demos.</p><p>I added you both to the sprint. <at id=\"0\">Tobias Klein</at> did you present everything about the migration tool? The spec is at <a href=\"https://wiki.example.com/display/TEAM/Settings+types+and+password+policies\">https://wiki.example.com/display/TEAM/Settings+types+and+password+policies</a>Can you please review it?</p>",
            "[]",
            false,
        ),
        message(
            channel,
            "t4r1",
            Some("t4"),
            tobias,
            at(0, 9, 15),
            "<p>Yes, all done.</p>",
            "[]",
            false,
        ),
    ];
    records.extend(rollout_replies);
    records
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut low, mut high) = (1u32, 0u32);
    for byte in bytes {
        low = (low + u32::from(*byte)) % 65521;
        high = (high + low) % 65521;
    }
    (high << 16) | low
}

fn png_chunk(output: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    output.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut body = kind.to_vec();
    body.extend_from_slice(data);
    output.extend_from_slice(&body);
    output.extend_from_slice(&crc32(&body).to_be_bytes());
}

fn inside(x: f32, y: f32, center: (f32, f32), radius: (f32, f32)) -> bool {
    ((x - center.0) / radius.0).powi(2) + ((y - center.1) / radius.1).powi(2) <= 1.
}

fn staging_pixel(column: usize, row: usize) -> [u8; 3] {
    let (width, height) = (STAGING_SIZE.0 as f32, STAGING_SIZE.1 as f32);
    let (x, y) = (column as f32, row as f32);
    let shade = y / height;
    let background = [
        (30. - 14. * shade) as u8,
        (41. - 16. * shade) as u8,
        (59. - 14. * shade) as u8,
    ];
    let bar_width = width / 14.;
    let slot = (x / bar_width) as usize;
    let in_bar = x - slot as f32 * bar_width > bar_width * 0.18;
    let bar_height = height * (0.25 + 0.5 * (((slot * 7 + 3) % 11) as f32 / 11.));
    let on_bars = in_bar && y > height * 0.86 - bar_height && y < height * 0.86 && slot < 14;
    let header = y < height * 0.1;
    if header {
        [22, 30, 45]
    } else if on_bars {
        if slot % 4 == 3 {
            [226, 87, 47]
        } else {
            [88, 133, 196]
        }
    } else if (y - height * 0.86).abs() < 1.5 {
        [71, 85, 105]
    } else {
        background
    }
}

fn encode_png(palette: &Palette) -> Vec<u8> {
    encode_rgb(PHOTO_SIZE, PHOTO_SIZE, |column, row| {
        let (x, y) = (column as f32 + 0.5, row as f32 + 0.5);
        if inside(x, y, (24., 21.), (8.5, 9.5)) {
            palette.skin
        } else if inside(x, y, (24., 19.), (10.5, 11.)) && y < 22. {
            palette.hair
        } else if inside(x, y, (24., 50.), (22., 19.)) {
            palette.shirt
        } else {
            palette.background
        }
    })
}

fn encode_rgb(width: usize, height: usize, pixel: impl Fn(usize, usize) -> [u8; 3]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(height * (width * 3 + 1));
    for row in 0..height {
        raw.push(0);
        for column in 0..width {
            raw.extend_from_slice(&pixel(column, row));
        }
    }
    let mut zlib = vec![0x78, 0x01];
    let mut blocks = raw.chunks(65535).peekable();
    while let Some(block) = blocks.next() {
        zlib.push(u8::from(blocks.peek().is_none()));
        zlib.extend_from_slice(&(block.len() as u16).to_le_bytes());
        zlib.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut header = Vec::new();
    header.extend_from_slice(&(width as u32).to_be_bytes());
    header.extend_from_slice(&(height as u32).to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    png_chunk(&mut png, b"IHDR", &header);
    png_chunk(&mut png, b"IDAT", &zlib);
    png_chunk(&mut png, b"IEND", &[]);
    png
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_has_signature_and_ends_with_iend() {
        let png = encode_png(&MARA);
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
        assert_eq!(&png[png.len() - 8..png.len() - 4], b"IEND");
    }

    #[test]
    fn crc_and_adler_match_known_values() {
        assert_eq!(crc32(b"IEND"), 0xae42_6082);
        assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
    }

    #[test]
    fn mention_candidates_filter_by_name_and_add_scope_in_channels() {
        let chat = mention_candidates("demo-chat-release", "ma");
        assert_eq!(chat.len(), 1);
        assert_eq!(chat[0].display_name(), "Mara Lindqvist");
        let channel = mention_candidates("demo-channel-1-1", "gen");
        assert!(matches!(channel[0], MentionCandidate::Channel { .. }));
        assert_eq!(mention_candidates("demo-chat-release", "").len(), 5);
    }

    #[test]
    fn new_group_chat_gets_a_title_and_the_first_message() {
        let people = vec![
            (MARA_ID.to_owned(), "Mara Lindqvist".to_owned()),
            (JONAS_ID.to_owned(), "Jonas Ortega".to_owned()),
        ];
        let now = Utc::now();
        let (chat, record) = new_chat(
            &[],
            &people,
            Some("Crew"),
            "Hello all",
            "Hello <i>all</i>",
            now,
        );
        assert_eq!(chat.id, "demo-chat-new-1");
        assert_eq!(chat.kind, "group");
        assert_eq!(chat.title, "Crew");
        assert_eq!(chat.member_summary, "Mara Lindqvist, Jonas Ortega");
        assert_eq!(chat.members.len(), 3);
        assert_eq!(chat.members[0].user_id.as_deref(), Some(DEMO_USER_ID));
        assert_eq!(chat.last_message_preview.as_deref(), Some("Hello all"));
        assert_eq!(chat.last_message_at, Some(now));
        assert_eq!(record.conversation_id, chat.id);
        assert_eq!(record.sender_id.as_deref(), Some(DEMO_USER_ID));
        assert!(record.body_html.contains("Hello"));
    }

    #[test]
    fn new_one_on_one_chat_has_no_title_and_counts_up() {
        let people = vec![(LEA_ID.to_owned(), "Lea Schneider".to_owned())];
        let (first, _) = new_chat(&[], &people, None, "Hi", "Hi", Utc::now());
        assert_eq!(first.kind, "oneOnOne");
        assert!(first.title.is_empty());
        let (second, _) = new_chat(&[first], &people, None, "Hi again", "Hi again", Utc::now());
        assert_eq!(second.id, "demo-chat-new-2");
    }

    #[test]
    fn sending_into_a_chat_refreshes_its_preview() {
        let base = ChatRecord {
            id: "demo-chat-x".to_owned(),
            unread: true,
            ..Default::default()
        };
        let (chat, record) = send_in_chat(&base, "Ping", "Ping", Utc::now());
        assert!(!chat.unread);
        assert_eq!(chat.last_message_preview.as_deref(), Some("Ping"));
        assert_eq!(record.conversation_id, "demo-chat-x");
    }

    #[test]
    fn demo_covers_the_sidebar_edge_cases() {
        let chats = demo_chats();
        assert!(chats.iter().any(|chat| chat.deleted));
        assert!(chats.iter().any(|chat| chat.title.len() > 40));
        assert!(chats.iter().filter(|chat| chat.unread).count() >= 3);
    }
}
