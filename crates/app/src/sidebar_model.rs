use std::collections::{HashMap, HashSet};

use chrono::{DateTime, FixedOffset, Utc};
use store::{ChatRecord, Sidebar};

use crate::app_state::chat_title;
use crate::data::{
    Directory, FolderKind, Person, face_members, is_one_on_one, others, unread_count,
};
use crate::format;
use crate::typing::{TypingState, preview_label};

pub const FAVORITES_FALLBACK_NAME: &str = "Pinned";
pub const OTHERS_NAME: &str = "Other chats";
pub const OTHERS_ID: &str = "others";
pub const EMPTY_FOLDER_HINT: &str = "Empty. Drag chats here.";
pub const DELETED_PREVIEW: &str = "Message deleted";
const OWN_PREFIX: &str = "You";
const GROUP_KIND: &str = "group";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Preview {
    Empty,
    Deleted,
    Typing(String),
    Text {
        prefix: Option<String>,
        text: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unread {
    None,
    Dot,
    Count(u32),
}

impl Unread {
    pub fn is_unread(self) -> bool {
        self != Unread::None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Face {
    pub user_id: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AvatarSpec {
    Single(Face),
    Pair(Face, Face),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatItem {
    pub id: String,
    pub title: String,
    pub time_label: String,
    pub preview: Preview,
    pub unread: Unread,
    pub muted: bool,
    pub is_group: bool,
    pub member_count: usize,
    pub avatar: AvatarSpec,
    pub presence_user: Option<String>,
    pub folder_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionKind {
    Favorites,
    Folder,
    Others,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub id: String,
    pub name: String,
    pub kind: SectionKind,
    pub collapsed: bool,
    pub count: usize,
    pub unread_chats: u32,
    pub items: Vec<ChatItem>,
    pub show_empty_hint: bool,
}

pub struct SectionInput<'a> {
    pub chats: &'a [ChatRecord],
    pub directory: &'a Directory,
    pub collapsed: &'a HashSet<String>,
    pub typing: &'a TypingState,
    pub now: DateTime<Utc>,
    pub offset: FixedOffset,
}

pub fn preview_for(chat: &ChatRecord, me: Option<&Person>) -> Preview {
    if chat.last_message_deleted {
        return Preview::Deleted;
    }
    let Some(text) = chat
        .last_message_preview
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    else {
        return Preview::Empty;
    };
    let from_me = match (chat.last_message_sender_id.as_deref(), me) {
        (Some(sender), Some(me)) => sender == me.user_id,
        _ => false,
    };
    let prefix = if from_me {
        Some(OWN_PREFIX.to_owned())
    } else if is_one_on_one(chat) {
        None
    } else {
        chat.last_message_sender_name
            .as_deref()
            .map(|name| format::first_name(name).to_owned())
            .filter(|name| !name.is_empty())
    };
    Preview::Text {
        prefix,
        text: text.to_owned(),
    }
}

pub fn avatar_for(chat: &ChatRecord, me: Option<&Person>) -> AvatarSpec {
    let faces = face_members(chat, me);
    let face = |(user_id, name): (Option<String>, String)| Face { user_id, name };
    if is_one_on_one(chat) || faces.len() < 2 {
        return AvatarSpec::Single(match faces.into_iter().next() {
            Some(first) => face(first),
            None => Face {
                user_id: None,
                name: chat_title(chat),
            },
        });
    }
    let mut iterator = faces.into_iter();
    let (first, second) = (iterator.next(), iterator.next());
    match (first, second) {
        (Some(first), Some(second)) => AvatarSpec::Pair(face(first), face(second)),
        _ => AvatarSpec::Single(Face {
            user_id: None,
            name: chat_title(chat),
        }),
    }
}

pub fn chat_item(chat: &ChatRecord, input: &SectionInput<'_>, folder_id: Option<&str>) -> ChatItem {
    let me = input.directory.me.as_ref();
    let unread = if chat.unread {
        unread_count(input.directory, chat)
            .filter(|count| *count > 0)
            .map_or(Unread::Dot, Unread::Count)
    } else {
        Unread::None
    };
    let presence_user = is_one_on_one(chat)
        .then(|| {
            others(chat, me)
                .into_iter()
                .find_map(|(user_id, _)| user_id)
        })
        .flatten();
    let typing_names = input.typing.names(&chat.id);
    let preview = if typing_names.is_empty() {
        preview_for(chat, me)
    } else {
        Preview::Typing(preview_label(&typing_names, is_one_on_one(chat)))
    };
    ChatItem {
        id: chat.id.clone(),
        title: chat_title(chat),
        time_label: chat
            .last_message_at
            .map(|time| {
                format::list_time_label(
                    time,
                    input.now.with_timezone(&input.offset).date_naive(),
                    input.offset,
                )
            })
            .unwrap_or_default(),
        preview,
        unread,
        muted: chat.muted,
        is_group: chat.kind == GROUP_KIND,
        member_count: chat.members.len(),
        avatar: avatar_for(chat, me),
        presence_user,
        folder_id: folder_id.map(str::to_owned),
    }
}

pub fn unread_chat_count(chats: &[ChatRecord]) -> u32 {
    chats
        .iter()
        .filter(|chat| chat.unread && !chat.muted)
        .count() as u32
}

pub fn any_unread_channel(sidebar: &Sidebar) -> bool {
    sidebar
        .teams
        .iter()
        .any(|team| team.channels.iter().any(|channel| channel.unread))
}

pub fn build_sections(input: &SectionInput<'_>) -> Vec<Section> {
    let by_id: HashMap<&str, &ChatRecord> = input
        .chats
        .iter()
        .map(|chat| (chat.id.as_str(), chat))
        .collect();
    let mut placed: HashSet<&str> = HashSet::new();
    let mut sections = Vec::new();
    for folder in &input.directory.folders {
        let items: Vec<ChatItem> = folder
            .conversation_ids
            .iter()
            .filter_map(|id| by_id.get(id.as_str()).copied())
            .filter(|chat| placed.insert(chat.id.as_str()))
            .map(|chat| chat_item(chat, input, Some(&folder.id)))
            .collect();
        let (kind, name) = match folder.kind {
            FolderKind::Favorites => (
                SectionKind::Favorites,
                if folder.name.trim().is_empty() {
                    FAVORITES_FALLBACK_NAME.to_owned()
                } else {
                    folder.name.clone()
                },
            ),
            FolderKind::UserCreated => (SectionKind::Folder, folder.name.clone()),
        };
        sections.push(section(folder.id.clone(), name, kind, items, input));
    }
    let rest: Vec<ChatItem> = input
        .chats
        .iter()
        .filter(|chat| !placed.contains(chat.id.as_str()))
        .map(|chat| chat_item(chat, input, None))
        .collect();
    sections.push(section(
        OTHERS_ID.to_owned(),
        OTHERS_NAME.to_owned(),
        SectionKind::Others,
        rest,
        input,
    ));
    sections
}

pub fn next_chat_id(sections: &[Section], removed_id: &str) -> Option<String> {
    let visible: Vec<&str> = sections
        .iter()
        .filter(|section| !section.collapsed)
        .flat_map(|section| section.items.iter().map(|item| item.id.as_str()))
        .collect();
    let position = visible.iter().position(|id| *id == removed_id)?;
    visible
        .get(position + 1)
        .or_else(|| {
            position
                .checked_sub(1)
                .and_then(|previous| visible.get(previous))
        })
        .map(|id| (*id).to_owned())
}

fn section(
    id: String,
    name: String,
    kind: SectionKind,
    items: Vec<ChatItem>,
    input: &SectionInput<'_>,
) -> Section {
    let collapsed = input.collapsed.contains(&id);
    let unread_chats = items
        .iter()
        .filter(|item| item.unread.is_unread() && !item.muted)
        .count() as u32;
    Section {
        collapsed,
        count: items.len(),
        unread_chats,
        show_empty_hint: items.is_empty() && !collapsed && kind != SectionKind::Others,
        id,
        name,
        kind,
        items,
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use store::MemberRecord;

    use super::*;
    use crate::data::FolderInfo;

    fn me() -> Person {
        Person {
            user_id: "me".into(),
            display_name: "Me Myself".into(),
        }
    }

    fn chat(id: &str, kind: &str, unread: bool) -> ChatRecord {
        ChatRecord {
            id: id.into(),
            kind: kind.into(),
            title: id.to_uppercase(),
            unread,
            last_message_at: Some(Utc.with_ymd_and_hms(2026, 10, 7, 9, 30, 0).unwrap()),
            members: vec![
                MemberRecord {
                    user_id: Some("me".into()),
                    display_name: "Me Myself".into(),
                },
                MemberRecord {
                    user_id: Some("ada".into()),
                    display_name: "Ada Example".into(),
                },
                MemberRecord {
                    user_id: Some("bob".into()),
                    display_name: "Bob Sample".into(),
                },
            ],
            ..Default::default()
        }
    }

    fn folder(id: &str, kind: FolderKind, ids: &[&str]) -> FolderInfo {
        FolderInfo {
            id: id.into(),
            name: format!("name-{id}"),
            kind,
            conversation_ids: ids.iter().map(|id| (*id).to_owned()).collect(),
        }
    }

    fn input<'a>(
        chats: &'a [ChatRecord],
        directory: &'a Directory,
        collapsed: &'a HashSet<String>,
    ) -> SectionInput<'a> {
        typing_input(chats, directory, collapsed, &NO_TYPING)
    }

    static NO_TYPING: std::sync::LazyLock<TypingState> =
        std::sync::LazyLock::new(TypingState::default);

    fn typing_input<'a>(
        chats: &'a [ChatRecord],
        directory: &'a Directory,
        collapsed: &'a HashSet<String>,
        typing: &'a TypingState,
    ) -> SectionInput<'a> {
        SectionInput {
            chats,
            directory,
            collapsed,
            typing,
            now: Utc.with_ymd_and_hms(2026, 10, 7, 12, 0, 0).unwrap(),
            offset: FixedOffset::east_opt(0).unwrap(),
        }
    }

    #[test]
    fn next_chat_is_the_following_row_then_the_previous_one() {
        let chats = [
            chat("a", "group", false),
            chat("b", "group", false),
            chat("c", "group", false),
        ];
        let directory = Directory::default();
        let collapsed = HashSet::new();
        let sections = build_sections(&input(&chats, &directory, &collapsed));
        assert_eq!(next_chat_id(&sections, "a"), Some("b".to_owned()));
        assert_eq!(next_chat_id(&sections, "c"), Some("b".to_owned()));
        assert_eq!(next_chat_id(&sections, "zzz"), None);
        let single = [chat("a", "group", false)];
        let sections = build_sections(&input(&single, &directory, &collapsed));
        assert_eq!(next_chat_id(&sections, "a"), None);
    }

    #[test]
    fn muted_chats_stay_out_of_unread_counts() {
        let mut quiet = chat("a", "group", true);
        quiet.muted = true;
        let chats = [quiet, chat("b", "group", true)];
        assert_eq!(unread_chat_count(&chats), 1);
        let directory = Directory::default();
        let collapsed = HashSet::new();
        let sections = build_sections(&input(&chats, &directory, &collapsed));
        assert_eq!(sections[0].unread_chats, 1);
        assert!(sections[0].items[0].muted);
        assert!(sections[0].items[0].unread.is_unread());
    }

    #[test]
    fn preview_prefix_is_you_for_own_and_first_name_for_groups() {
        let mut group = chat("g", "group", false);
        group.last_message_preview = Some("Hello".into());
        group.last_message_sender_id = Some("ada".into());
        group.last_message_sender_name = Some("Ada Example".into());
        assert_eq!(
            preview_for(&group, Some(&me())),
            Preview::Text {
                prefix: Some("Ada".into()),
                text: "Hello".into()
            }
        );
        group.last_message_sender_id = Some("me".into());
        assert_eq!(
            preview_for(&group, Some(&me())),
            Preview::Text {
                prefix: Some("You".into()),
                text: "Hello".into()
            }
        );
        let mut direct = chat("d", "oneOnOne", false);
        direct.last_message_preview = Some("Hi".into());
        direct.last_message_sender_id = Some("ada".into());
        direct.last_message_sender_name = Some("Ada Example".into());
        assert_eq!(
            preview_for(&direct, Some(&me())),
            Preview::Text {
                prefix: None,
                text: "Hi".into()
            }
        );
    }

    #[test]
    fn deleted_and_blank_previews() {
        let mut deleted = chat("d", "group", false);
        deleted.last_message_deleted = true;
        deleted.last_message_preview = Some("secret".into());
        assert_eq!(preview_for(&deleted, None), Preview::Deleted);
        assert_eq!(
            preview_for(&chat("e", "group", false), None),
            Preview::Empty
        );
    }

    #[test]
    fn sections_follow_folder_order_and_leave_the_rest_for_others() {
        let chats = vec![
            chat("a", "group", false),
            chat("b", "oneOnOne", true),
            chat("c", "group", false),
            chat("d", "group", false),
        ];
        let directory = Directory {
            me: Some(me()),
            folders: vec![
                folder("fav", FolderKind::Favorites, &["c"]),
                folder("work", FolderKind::UserCreated, &["b", "gone", "a"]),
                folder("todo", FolderKind::UserCreated, &[]),
            ],
            ..Default::default()
        };
        let collapsed = HashSet::new();
        let sections = build_sections(&input(&chats, &directory, &collapsed));
        let ids: Vec<&str> = sections.iter().map(|section| section.id.as_str()).collect();
        assert_eq!(ids, vec!["fav", "work", "todo", OTHERS_ID]);
        let work: Vec<&str> = sections[1]
            .items
            .iter()
            .map(|item| item.id.as_str())
            .collect();
        assert_eq!(work, vec!["b", "a"]);
        assert_eq!(sections[1].count, 2);
        assert_eq!(sections[2].count, 0);
        assert!(sections[2].show_empty_hint);
        let others: Vec<&str> = sections[3]
            .items
            .iter()
            .map(|item| item.id.as_str())
            .collect();
        assert_eq!(others, vec!["d"]);
        assert!(!sections[3].show_empty_hint);
    }

    #[test]
    fn collapsed_sections_keep_the_aggregated_unread() {
        let chats = vec![
            chat("a", "group", true),
            chat("b", "group", true),
            chat("c", "group", false),
        ];
        let directory = Directory {
            folders: vec![folder("work", FolderKind::UserCreated, &["a", "b", "c"])],
            ..Default::default()
        };
        let collapsed: HashSet<String> = ["work".to_owned()].into();
        let sections = build_sections(&input(&chats, &directory, &collapsed));
        assert!(sections[0].collapsed);
        assert_eq!(sections[0].unread_chats, 2);
        assert_eq!(sections[0].count, 3);
        assert!(!sections[0].show_empty_hint);
    }

    #[test]
    fn collapsed_empty_folder_hides_the_hint() {
        let directory = Directory {
            folders: vec![folder("todo", FolderKind::UserCreated, &[])],
            ..Default::default()
        };
        let collapsed: HashSet<String> = ["todo".to_owned()].into();
        let sections = build_sections(&input(&[], &directory, &collapsed));
        assert!(!sections[0].show_empty_hint);
    }

    #[test]
    fn favorites_without_a_server_name_say_angeheftet() {
        let mut favorites = folder("fav", FolderKind::Favorites, &[]);
        favorites.name = String::new();
        let directory = Directory {
            folders: vec![favorites],
            ..Default::default()
        };
        let sections = build_sections(&input(&[], &directory, &HashSet::new()));
        assert_eq!(sections[0].name, FAVORITES_FALLBACK_NAME);
    }

    #[test]
    fn unread_is_a_count_when_known_else_a_dot() {
        let chats = vec![
            chat("a", "group", true),
            chat("b", "group", true),
            chat("c", "group", false),
        ];
        let mut directory = Directory::default();
        directory.unread_counts.insert("a".into(), 5);
        let collapsed = HashSet::new();
        let context = input(&chats, &directory, &collapsed);
        assert_eq!(
            chat_item(&chats[0], &context, None).unread,
            Unread::Count(5)
        );
        assert_eq!(chat_item(&chats[1], &context, None).unread, Unread::Dot);
        assert_eq!(chat_item(&chats[2], &context, None).unread, Unread::None);
        assert_eq!(unread_chat_count(&chats), 2);
    }

    #[test]
    fn one_on_one_has_presence_and_a_single_face_groups_have_a_pair() {
        let directory = Directory {
            me: Some(me()),
            ..Default::default()
        };
        let collapsed = HashSet::new();
        let chats = vec![chat("d", "oneOnOne", false), chat("g", "group", false)];
        let context = input(&chats, &directory, &collapsed);
        let direct = chat_item(&chats[0], &context, None);
        assert_eq!(direct.presence_user.as_deref(), Some("ada"));
        assert!(
            matches!(direct.avatar, AvatarSpec::Single(ref face) if face.name == "Ada Example")
        );
        let group = chat_item(&chats[1], &context, None);
        assert_eq!(group.presence_user, None);
        assert!(matches!(group.avatar, AvatarSpec::Pair(..)));
    }

    #[test]
    fn typing_replaces_the_preview_even_in_muted_chats() {
        let mut direct = chat("d", "oneOnOne", false);
        direct.last_message_preview = Some("Earlier".into());
        direct.muted = true;
        let group = chat("g", "group", false);
        let chats = vec![direct, group];
        let directory = Directory::default();
        let collapsed = HashSet::new();
        let now = std::time::Instant::now();
        let mut typing = TypingState::default();
        typing.start("d", "ada", "Ada Example", now, Utc::now());
        typing.start("g", "ada", "Ada Example", now, Utc::now());
        let context = typing_input(&chats, &directory, &collapsed, &typing);
        assert_eq!(
            chat_item(&chats[0], &context, None).preview,
            Preview::Typing("typing...".into())
        );
        assert_eq!(
            chat_item(&chats[1], &context, None).preview,
            Preview::Typing("Ada is typing...".into())
        );
        typing.clear("d", "ada");
        let context = typing_input(&chats, &directory, &collapsed, &typing);
        assert_eq!(
            chat_item(&chats[0], &context, None).preview,
            Preview::Text {
                prefix: None,
                text: "Earlier".into()
            }
        );
    }

    #[test]
    fn time_label_uses_the_list_rules() {
        let chats = vec![chat("a", "group", false)];
        let directory = Directory::default();
        let collapsed = HashSet::new();
        let item = chat_item(&chats[0], &input(&chats, &directory, &collapsed), None);
        assert_eq!(item.time_label, "09:30");
    }
}
