use std::collections::HashMap;
use std::time::{Duration, Instant};

use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use store::MessageRecord;
use teams_core::{
    DriveEntry, DriveFolder, FileCard, LibraryFile, files, preview_text, public_links,
};

use crate::format;

const WEEK_DAYS: i64 = 6;
const EXCERPT_WORDS: usize = 6;
const DOWNLOAD_URL_LIFETIME: Duration = Duration::from_secs(50 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadSource {
    Share,
    Library(LibraryFile),
}

pub fn library_file(entry: &DriveEntry, listed_at: Option<Instant>, now: Instant) -> LibraryFile {
    let fresh = listed_at
        .is_some_and(|listed_at| now.saturating_duration_since(listed_at) < DOWNLOAD_URL_LIFETIME);
    LibraryFile {
        drive_id: entry.drive_id.clone(),
        item_id: entry.id.clone(),
        name: entry.name.clone(),
        size: entry.size,
        cached_url: entry.download_url.clone().filter(|_| fresh),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crumb {
    pub name: String,
    pub folder: DriveFolder,
    pub web_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryPath {
    crumbs: Vec<Crumb>,
}

impl LibraryPath {
    pub fn new(root: Crumb) -> Self {
        LibraryPath { crumbs: vec![root] }
    }

    pub fn crumbs(&self) -> &[Crumb] {
        &self.crumbs
    }

    pub fn current(&self) -> &DriveFolder {
        &self.crumbs[self.crumbs.len() - 1].folder
    }

    pub fn current_web_url(&self) -> &str {
        &self.crumbs[self.crumbs.len() - 1].web_url
    }

    pub fn enter(&mut self, entry: &DriveEntry) {
        self.crumbs.push(Crumb {
            name: entry.name.clone(),
            folder: entry.folder(),
            web_url: entry.web_url.clone(),
        });
    }

    pub fn go_to(&mut self, index: usize) {
        self.crumbs.truncate(index + 1);
    }
}

pub fn count_label(child_count: u64) -> String {
    match child_count {
        1 => "1 item".to_owned(),
        count => format!("{count} items"),
    }
}

pub fn size_label(entry: &DriveEntry) -> String {
    match entry.child_count {
        Some(count) => count_label(count),
        None => format::file_size_label(entry.size),
    }
}

pub fn sorted_entries(mut entries: Vec<DriveEntry>) -> Vec<DriveEntry> {
    entries.sort_by_cached_key(|entry| (!entry.is_folder(), entry.name.to_lowercase()));
    entries
}

pub fn file_card(entry: &DriveEntry) -> FileCard {
    FileCard {
        kind: teams_core::FileKind::from_name(&entry.name),
        name: entry.name.clone(),
        content_type: None,
        size: Some(entry.size),
        open_url: entry.web_url.clone(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageFilter {
    All,
    Files,
    Links,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SharedContent {
    File(FileCard),
    Link(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedItem {
    pub content: SharedContent,
    pub author: String,
    pub post_label: String,
    pub message_id: String,
    pub created_at: DateTime<Utc>,
}

impl SharedItem {
    pub fn name(&self) -> &str {
        match &self.content {
            SharedContent::File(card) => &card.name,
            SharedContent::Link(url) => url,
        }
    }

    fn matches(&self, filter: MessageFilter) -> bool {
        matches!(
            (filter, &self.content),
            (MessageFilter::All, _)
                | (MessageFilter::Files, SharedContent::File(_))
                | (MessageFilter::Links, SharedContent::Link(_))
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharedGroup {
    ThisWeek,
    Earlier,
}

impl SharedGroup {
    pub fn label(self) -> &'static str {
        match self {
            SharedGroup::ThisWeek => "This week",
            SharedGroup::Earlier => "Earlier",
        }
    }
}

fn post_label(root: Option<&MessageRecord>) -> String {
    let Some(root) = root else {
        return String::new();
    };
    if let Some(subject) = root
        .subject
        .as_deref()
        .filter(|subject| !subject.is_empty())
    {
        return subject.to_owned();
    }
    let text = preview_text(&root.body_html).unwrap_or_default();
    let words: Vec<&str> = text.split_whitespace().collect();
    let shown = words[..words.len().min(EXCERPT_WORDS)].join(" ");
    if words.len() > EXCERPT_WORDS {
        format!("{shown} ...")
    } else {
        shown
    }
}

pub fn shared_items(records: &[MessageRecord]) -> Vec<SharedItem> {
    let roots: HashMap<&str, &MessageRecord> = records
        .iter()
        .filter(|record| record.reply_to_id.is_none())
        .map(|record| (record.message_id.as_str(), record))
        .collect();
    let mut items = Vec::new();
    for record in records.iter().filter(|record| !record.deleted) {
        let root = match record.reply_to_id.as_deref() {
            Some(root_id) => roots.get(root_id).copied(),
            None => Some(record),
        };
        let label = post_label(root);
        let author = record.sender_name.clone().unwrap_or_default();
        let contents = files(record)
            .into_iter()
            .map(SharedContent::File)
            .chain(public_links(record).into_iter().map(SharedContent::Link));
        items.extend(contents.map(|content| SharedItem {
            content,
            author: author.clone(),
            post_label: label.clone(),
            message_id: record.message_id.clone(),
            created_at: record.created_at,
        }));
    }
    items.sort_by_key(|item| std::cmp::Reverse(item.created_at));
    items
}

pub fn group_of(created_at: DateTime<Utc>, today: NaiveDate, offset: FixedOffset) -> SharedGroup {
    let day = created_at.with_timezone(&offset).date_naive();
    if (today - day).num_days() <= WEEK_DAYS {
        SharedGroup::ThisWeek
    } else {
        SharedGroup::Earlier
    }
}

pub fn grouped_items(
    items: &[SharedItem],
    filter: MessageFilter,
    today: NaiveDate,
    offset: FixedOffset,
) -> Vec<(SharedGroup, Vec<usize>)> {
    let mut groups: Vec<(SharedGroup, Vec<usize>)> = Vec::new();
    for (index, item) in items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.matches(filter))
    {
        let group = group_of(item.created_at, today, offset);
        match groups.iter_mut().find(|(known, _)| *known == group) {
            Some((_, members)) => members.push(index),
            None => groups.push((group, vec![index])),
        }
    }
    groups.sort_by_key(|(group, _)| *group as u8);
    groups
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn folder(id: &str) -> DriveFolder {
        DriveFolder {
            drive_id: "drive".to_owned(),
            item_id: id.to_owned(),
        }
    }

    fn entry(name: &str, child_count: Option<u64>, size: u64) -> DriveEntry {
        DriveEntry {
            drive_id: "drive".to_owned(),
            id: format!("id-{name}"),
            name: name.to_owned(),
            web_url: format!("https://x/{name}"),
            size,
            modified_at: None,
            modified_by: None,
            child_count,
            download_url: None,
        }
    }

    fn record(id: &str, reply_to: Option<&str>, days_ago: i64, html: &str) -> MessageRecord {
        MessageRecord {
            conversation_id: "channel".to_owned(),
            message_id: id.to_owned(),
            reply_to_id: reply_to.map(str::to_owned),
            sender_id: None,
            sender_name: Some("Mara".to_owned()),
            created_at: Utc.with_ymd_and_hms(2026, 10, 9, 12, 0, 0).unwrap()
                - chrono::Duration::days(days_ago),
            edited_at: None,
            deleted: false,
            body_html: html.to_owned(),
            attachments_json: "[]".to_owned(),
            reactions_json: "[]".to_owned(),
            mentions_json: "[]".to_owned(),
            sender_application_id: None,
            links_json: "[]".to_owned(),
            subject: None,
        }
    }

    fn with_file(mut record: MessageRecord, name: &str) -> MessageRecord {
        record.attachments_json = format!(
            r#"[{{"content_type":"reference","name":"{name}","url":"https://x/{name}","text":null,"size":10}}]"#
        );
        record
    }

    fn today() -> (NaiveDate, FixedOffset) {
        let offset = FixedOffset::east_opt(0).unwrap();
        (NaiveDate::from_ymd_opt(2026, 10, 9).unwrap(), offset)
    }

    #[test]
    fn a_listed_download_url_is_used_until_it_is_about_to_expire() {
        let mut listed = entry("a.pdf", None, 5);
        listed.download_url = Some("https://dl.example/a".to_owned());
        let start = Instant::now();
        let url = |age_minutes: u64| {
            library_file(
                &listed,
                Some(start),
                start + Duration::from_secs(age_minutes * 60),
            )
            .cached_url
        };
        assert_eq!(url(10).as_deref(), Some("https://dl.example/a"));
        assert_eq!(url(49).as_deref(), Some("https://dl.example/a"));
        assert_eq!(url(51), None);
        assert_eq!(library_file(&listed, None, start).cached_url, None);
        let without = entry("b.pdf", None, 5);
        assert_eq!(library_file(&without, Some(start), start).cached_url, None);
    }

    #[test]
    fn breadcrumb_path_grows_and_shrinks() {
        let mut path = LibraryPath::new(Crumb {
            name: "Channel".to_owned(),
            folder: folder("root"),
            web_url: "https://x/root".to_owned(),
        });
        path.enter(&entry("Specs", Some(2), 0));
        path.enter(&entry("2026", Some(0), 0));
        let names: Vec<&str> = path
            .crumbs()
            .iter()
            .map(|crumb| crumb.name.as_str())
            .collect();
        assert_eq!(names, ["Channel", "Specs", "2026"]);
        assert_eq!(path.current().item_id, "id-2026");
        path.go_to(1);
        assert_eq!(path.current().item_id, "id-Specs");
        path.go_to(0);
        assert_eq!(path.current(), &folder("root"));
        assert_eq!(path.current_web_url(), "https://x/root");
    }

    #[test]
    fn sizes_and_item_counts_read_like_teams() {
        assert_eq!(size_label(&entry("a", Some(0), 0)), "0 items");
        assert_eq!(size_label(&entry("a", Some(1), 0)), "1 item");
        assert_eq!(size_label(&entry("a", Some(12), 0)), "12 items");
        assert_eq!(size_label(&entry("a.pdf", None, 48_213)), "47 KB");
    }

    #[test]
    fn folders_sort_before_files_by_name() {
        let sorted = sorted_entries(vec![
            entry("b.pdf", None, 1),
            entry("Zeta", Some(0), 0),
            entry("a.pdf", None, 1),
            entry("alpha", Some(0), 0),
        ]);
        let names: Vec<&str> = sorted.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["alpha", "Zeta", "a.pdf", "b.pdf"]);
    }

    #[test]
    fn messages_group_into_this_week_and_earlier() {
        let records = vec![
            with_file(record("old", None, 20, "<p>Old plan</p>"), "Old.pdf"),
            with_file(record("new", None, 1, "<p>New plan</p>"), "New.xlsx"),
            record(
                "link",
                None,
                2,
                "<p><a href=\"https://a.example/x\">x</a></p>",
            ),
        ];
        let items = shared_items(&records);
        let (day, offset) = today();
        let groups = grouped_items(&items, MessageFilter::All, day, offset);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].0, SharedGroup::ThisWeek);
        let week: Vec<&str> = groups[0]
            .1
            .iter()
            .map(|index| items[*index].name())
            .collect();
        assert_eq!(week, ["New.xlsx", "https://a.example/x"]);
        assert_eq!(groups[1].0, SharedGroup::Earlier);
        assert_eq!(items[groups[1].1[0]].name(), "Old.pdf");
    }

    #[test]
    fn filter_chips_split_files_from_links() {
        let records = vec![
            with_file(record("a", None, 1, "<p>Plan</p>"), "Plan.pdf"),
            record("b", None, 1, "<p><a href=\"https://a.example\">a</a></p>"),
        ];
        let items = shared_items(&records);
        let (day, offset) = today();
        let count = |filter| -> usize {
            grouped_items(&items, filter, day, offset)
                .iter()
                .map(|(_, members)| members.len())
                .sum()
        };
        assert_eq!(count(MessageFilter::All), 2);
        assert_eq!(count(MessageFilter::Files), 1);
        assert_eq!(count(MessageFilter::Links), 1);
    }

    #[test]
    fn replies_name_their_post_by_subject_or_first_words() {
        let mut root = record(
            "root",
            None,
            1,
            "<p>One two three four five six seven eight</p>",
        );
        let reply = with_file(record("reply", Some("root"), 1, "<p>See file</p>"), "A.pdf");
        let subject_root = {
            let mut other = record("other", None, 1, "<p>Body</p>");
            other.subject = Some("Release 42".to_owned());
            other
        };
        let other_reply = with_file(record("r2", Some("other"), 1, "<p>x</p>"), "B.pdf");
        root.sender_name = Some("Tobias".to_owned());
        let items = shared_items(&[root, reply, subject_root, other_reply]);
        let label_of = |name: &str| {
            items
                .iter()
                .find(|item| item.name() == name)
                .map(|item| item.post_label.clone())
        };
        assert_eq!(
            label_of("A.pdf").as_deref(),
            Some("One two three four five six ...")
        );
        assert_eq!(label_of("B.pdf").as_deref(), Some("Release 42"));
        assert_eq!(
            items
                .iter()
                .find(|item| item.name() == "A.pdf")
                .unwrap()
                .message_id,
            "reply"
        );
    }
}
