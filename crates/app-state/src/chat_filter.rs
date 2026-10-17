use std::collections::HashSet;

use store::ChatRecord;

use crate::activity::{Entry, Kind};
use crate::app_state::chat_title;
use crate::sidebar_model::{MUTED_ID, OTHERS_ID, Section, SectionInput, build_sections};

pub const FILTER_PLACEHOLDER: &str = "Filter by person or chat name";
pub const NO_MATCHES: &str = "We didn't find any matches.";
pub const CLEAR_FILTERS: &str = "Clear filters";
pub const CLEAR_FILTER_TEXT: &str = "Clear filter text";
pub const SHOW_FILTER_TEXT: &str = "Show filter text box";
pub const CLOSE_FILTER_TEXT: &str = "Close filter text box";
pub const MORE_FILTERS: &str = "More filters";
pub const FEWER_FILTERS: &str = "Fewer filters";
const MEETING_KIND: &str = "meeting";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChatFilterPill {
    Unread,
    Mentions,
    Chats,
    Meetings,
    Muted,
}

impl ChatFilterPill {
    pub const PRIMARY: [ChatFilterPill; 3] = [
        ChatFilterPill::Unread,
        ChatFilterPill::Mentions,
        ChatFilterPill::Chats,
    ];
    pub const OVERFLOW: [ChatFilterPill; 2] = [ChatFilterPill::Meetings, ChatFilterPill::Muted];

    pub fn label(self) -> &'static str {
        match self {
            ChatFilterPill::Unread => "Unread",
            ChatFilterPill::Mentions => "@Mentions",
            ChatFilterPill::Chats => "Chats",
            ChatFilterPill::Meetings => "Meeting chats",
            ChatFilterPill::Muted => "Muted",
        }
    }

    fn is_category(self) -> bool {
        matches!(self, ChatFilterPill::Chats | ChatFilterPill::Meetings)
    }

    fn excludes(self) -> Option<ChatFilterPill> {
        match self {
            ChatFilterPill::Mentions => Some(ChatFilterPill::Muted),
            ChatFilterPill::Muted => Some(ChatFilterPill::Mentions),
            ChatFilterPill::Chats => Some(ChatFilterPill::Meetings),
            ChatFilterPill::Meetings => Some(ChatFilterPill::Chats),
            ChatFilterPill::Unread => None,
        }
    }

    fn admits(self, chat: &ChatRecord, mentioned: &HashSet<String>) -> bool {
        let meeting = chat.kind.eq_ignore_ascii_case(MEETING_KIND);
        match self {
            ChatFilterPill::Unread => chat.unread,
            ChatFilterPill::Mentions => mentioned.contains(&chat.id),
            ChatFilterPill::Chats => !meeting,
            ChatFilterPill::Meetings => meeting,
            ChatFilterPill::Muted => chat.muted,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChatFilter {
    pub text: String,
    pub pills: Vec<ChatFilterPill>,
}

impl ChatFilter {
    pub fn is_active(&self) -> bool {
        !self.text.trim().is_empty() || !self.pills.is_empty()
    }

    pub fn has(&self, pill: ChatFilterPill) -> bool {
        self.pills.contains(&pill)
    }

    pub fn is_disabled(&self, pill: ChatFilterPill) -> bool {
        !self.has(pill)
            && self
                .pills
                .iter()
                .any(|active| active.excludes() == Some(pill))
    }

    pub fn toggle(&mut self, pill: ChatFilterPill) -> bool {
        if self.has(pill) {
            self.pills.retain(|active| *active != pill);
            return true;
        }
        if self.is_disabled(pill) {
            return false;
        }
        self.pills.push(pill);
        true
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.pills.clear();
    }

    pub fn admits(&self, chat: &ChatRecord, mentioned: &HashSet<String>) -> bool {
        if !name_matches(&self.text, &chat_title(chat)) {
            return false;
        }
        let states_hold = self
            .pills
            .iter()
            .filter(|pill| !pill.is_category())
            .all(|pill| pill.admits(chat, mentioned));
        let mut categories = self
            .pills
            .iter()
            .filter(|pill| pill.is_category())
            .peekable();
        let category_holds =
            categories.peek().is_none() || categories.any(|pill| pill.admits(chat, mentioned));
        states_hold && category_holds
    }
}

pub fn mentioned_conversations<'a>(entries: impl Iterator<Item = &'a Entry>) -> HashSet<String> {
    entries
        .filter(|entry| entry.kind == Kind::Mention)
        .map(|entry| entry.conversation_id.clone())
        .collect()
}

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|character: char| character.is_whitespace() || character == ',')
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

pub fn name_matches(query: &str, title: &str) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    let title_lower = title.to_lowercase();
    if title_lower.contains(&query) {
        return true;
    }
    let title_words = words(&title_lower);
    words(&query).iter().all(|word| {
        title_words
            .iter()
            .any(|title_word| title_word.starts_with(word.as_str()))
    })
}

pub fn filtered_sections(
    input: &SectionInput<'_>,
    filter: &ChatFilter,
    mentioned: &HashSet<String>,
    selected_id: Option<&str>,
) -> Vec<Section> {
    let chats: Vec<ChatRecord> = input
        .chats
        .iter()
        .filter(|chat| Some(chat.id.as_str()) == selected_id || filter.admits(chat, mentioned))
        .cloned()
        .collect();
    let every_list: HashSet<String> = input
        .directory
        .folders
        .iter()
        .map(|folder| folder.id.clone())
        .chain([MUTED_ID.to_owned(), OTHERS_ID.to_owned()])
        .collect();
    let unpaged = SectionInput {
        chats: &chats,
        expanded_lists: &every_list,
        ..*input
    };
    build_sections(&unpaged)
        .into_iter()
        .filter(|section| !section.items.is_empty())
        .map(|section| Section {
            hidden_count: 0,
            list_expanded: false,
            show_empty_hint: false,
            ..section
        })
        .collect()
}

pub fn has_no_matches(sections: &[Section], selected_id: Option<&str>) -> bool {
    sections
        .iter()
        .flat_map(|section| &section.items)
        .all(|item| Some(item.id.as_str()) == selected_id)
}

#[cfg(test)]
mod tests {
    use chrono::{FixedOffset, TimeZone, Utc};

    use super::*;
    use crate::data::{Directory, FolderInfo, FolderKind};
    use crate::typing::TypingState;
    use teams_core::ChatSection;

    fn chat(id: &str, title: &str, kind: &str) -> ChatRecord {
        ChatRecord {
            id: id.into(),
            kind: kind.into(),
            title: title.into(),
            ..Default::default()
        }
    }

    fn admitted(filter: &ChatFilter, chats: &[ChatRecord], mentioned: &[&str]) -> Vec<String> {
        let mentioned: HashSet<String> = mentioned.iter().map(|id| (*id).to_owned()).collect();
        chats
            .iter()
            .filter(|chat| filter.admits(chat, &mentioned))
            .map(|chat| chat.id.clone())
            .collect()
    }

    #[test]
    fn name_filter_matches_a_substring_or_every_word_prefix_like_teams() {
        assert!(name_matches("", "Anything"));
        assert!(name_matches("lind", "Mara Lindqvist"));
        assert!(name_matches("ma li", "Mara Lindqvist"));
        assert!(name_matches("LI, MA", "Mara Lindqvist"));
        assert!(!name_matches("ma xo", "Mara Lindqvist"));
    }

    #[test]
    fn state_pills_must_all_hold_and_category_pills_need_one() {
        let chats = [
            ChatRecord {
                unread: true,
                ..chat("unread", "Unread chat", "group")
            },
            ChatRecord {
                unread: true,
                ..chat("meeting", "Standup", "meeting")
            },
            ChatRecord {
                muted: true,
                ..chat("muted", "Muted chat", "group")
            },
            chat("plain", "Plain chat", "oneOnOne"),
        ];
        let mut filter = ChatFilter::default();
        assert_eq!(admitted(&filter, &chats, &[]).len(), 4);
        filter.toggle(ChatFilterPill::Unread);
        assert_eq!(admitted(&filter, &chats, &[]), ["unread", "meeting"]);
        filter.toggle(ChatFilterPill::Meetings);
        assert_eq!(admitted(&filter, &chats, &[]), ["meeting"]);
        filter.toggle(ChatFilterPill::Unread);
        filter.toggle(ChatFilterPill::Meetings);
        filter.toggle(ChatFilterPill::Chats);
        assert_eq!(admitted(&filter, &chats, &[]), ["unread", "muted", "plain"]);
        filter.toggle(ChatFilterPill::Mentions);
        assert_eq!(admitted(&filter, &chats, &["plain"]), ["plain"]);
    }

    #[test]
    fn exclusive_pills_disable_each_other() {
        let mut filter = ChatFilter::default();
        assert!(filter.toggle(ChatFilterPill::Chats));
        assert!(filter.is_disabled(ChatFilterPill::Meetings));
        assert!(!filter.toggle(ChatFilterPill::Meetings));
        assert!(filter.toggle(ChatFilterPill::Muted));
        assert!(filter.is_disabled(ChatFilterPill::Mentions));
        assert!(!filter.is_disabled(ChatFilterPill::Unread));
        assert!(filter.toggle(ChatFilterPill::Chats));
        assert!(!filter.is_disabled(ChatFilterPill::Meetings));
        filter.text = "x".into();
        assert!(filter.is_active());
        filter.clear();
        assert!(!filter.is_active());
    }

    #[test]
    fn text_combines_with_pills() {
        let chats = [
            ChatRecord {
                unread: true,
                ..chat("a", "Release crew", "group")
            },
            chat("b", "Release notes", "group"),
        ];
        let filter = ChatFilter {
            text: "rel".into(),
            pills: vec![ChatFilterPill::Unread],
        };
        assert_eq!(admitted(&filter, &chats, &[]), ["a"]);
    }

    static NO_TYPING: std::sync::LazyLock<TypingState> =
        std::sync::LazyLock::new(TypingState::default);

    #[test]
    fn filtered_sections_keep_sections_drop_empty_ones_and_unpage_muted() {
        let mut chats = vec![
            chat("fav", "Favorite release", "group"),
            chat("other", "Other release", "group"),
            chat("quiet", "Quiet", "group"),
        ];
        chats.extend((0..7).map(|index| ChatRecord {
            muted: true,
            ..chat(
                &format!("m{index}"),
                &format!("Muted release {index}"),
                "group",
            )
        }));
        let mut directory = Directory {
            folders: vec![
                FolderInfo {
                    id: "favorites".into(),
                    name: "Favorites".into(),
                    kind: FolderKind::Favorites,
                    expanded: None,
                    conversation_ids: vec!["fav".into()],
                },
                FolderInfo {
                    id: "empty".into(),
                    name: "Empty".into(),
                    kind: FolderKind::UserCreated,
                    expanded: None,
                    conversation_ids: Vec::new(),
                },
            ],
            ..Default::default()
        };
        directory.section_settings.set(ChatSection::Muted, true);
        let collapsed = HashSet::new();
        let expanded = HashSet::new();
        let input = SectionInput {
            chats: &chats,
            directory: &directory,
            collapsed: &collapsed,
            expanded_lists: &expanded,
            typing: &NO_TYPING,
            now: Utc.with_ymd_and_hms(2026, 10, 17, 12, 0, 0).unwrap(),
            offset: FixedOffset::east_opt(0).unwrap(),
        };
        let filter = ChatFilter {
            text: "release".into(),
            pills: Vec::new(),
        };
        let sections = filtered_sections(&input, &filter, &HashSet::new(), None);
        let names: Vec<(&str, usize)> = sections
            .iter()
            .map(|section| (section.id.as_str(), section.items.len()))
            .collect();
        assert_eq!(names, [("favorites", 1), (OTHERS_ID, 1), (MUTED_ID, 7)]);
        assert!(sections.iter().all(|section| section.hidden_count == 0
            && !section.list_expanded
            && !section.show_empty_hint));
        let with_selection = filtered_sections(&input, &filter, &HashSet::new(), Some("quiet"));
        assert!(
            with_selection
                .iter()
                .any(|section| section.items.iter().any(|item| item.id == "quiet"))
        );
    }
}
