use std::collections::HashSet;
use std::ops::Range;

use chatsvc::substrate_search::{MessageFilter, SearchMessage, split_highlights};
use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, Utc};
use store::{HIGHLIGHT_END, HIGHLIGHT_START, MessageRecord, SearchHit};

pub const FIND_IN_CHAT: &str = "Find in chat";
pub const FIND_IN_CHANNEL: &str = "Find in channel";
pub const EMPTY_HINT: &str = "Find messages and links shared in this chat.";
pub const NO_RESULTS_CHAT: &str = "We couldn't find any results in this chat";
pub const NO_RESULTS_CHANNEL: &str = "We couldn't find any results in this channel";
pub const LOAD_ERROR: &str = "Something went wrong while loading the results.";
pub const LOADING: &str = "Fetching Data...";
pub const FROM_LABEL: &str = "From";
pub const DATE_LABEL: &str = "Date";
pub const HAS_ATTACHMENT_LABEL: &str = "Has attachment";
pub const MENTIONS_LABEL: &str = "@Mentions";
pub const THIS_MONTH: &str = "This month";
pub const LAST_MONTH: &str = "Last month";
const ORG_PREFIX: &str = "8:orgid:";

pub fn no_results_with_filters(search_term: &str) -> String {
    format!("We couldn't find any results for '{search_term}' with the applied filters.")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindHit {
    pub message_id: String,
    pub sent_at: DateTime<Utc>,
    pub sender_name: String,
    pub sender_id: Option<String>,
    pub text: String,
    pub highlights: Vec<Range<usize>>,
    pub mentions_me: bool,
    pub has_attachment: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateRange {
    Today,
    Yesterday,
    ThisWeek,
    ThisMonth,
}

impl DateRange {
    pub const ALL: [DateRange; 4] = [
        DateRange::Today,
        DateRange::Yesterday,
        DateRange::ThisWeek,
        DateRange::ThisMonth,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DateRange::Today => "Today",
            DateRange::Yesterday => "Yesterday",
            DateRange::ThisWeek => "This week",
            DateRange::ThisMonth => "This month",
        }
    }

    pub fn contains(self, date: NaiveDate, today: NaiveDate) -> bool {
        match self {
            DateRange::Today => date == today,
            DateRange::Yesterday => Some(date) == today.pred_opt(),
            DateRange::ThisWeek => {
                let week_start =
                    today - Duration::days(i64::from(today.weekday().num_days_from_monday()));
                date >= week_start && date <= today
            }
            DateRange::ThisMonth => date.year() == today.year() && date.month() == today.month(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sender {
    pub user_id: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FindFilters {
    pub from: Option<Sender>,
    pub date: Option<DateRange>,
    pub has_attachment: bool,
    pub mentions_me: bool,
}

impl FindFilters {
    pub fn is_active(&self) -> bool {
        self.from.is_some() || self.date.is_some() || self.has_attachment || self.mentions_me
    }

    pub fn admits(&self, hit: &FindHit, today: NaiveDate, offset: FixedOffset) -> bool {
        let from_holds = self.from.as_ref().is_none_or(|sender| {
            hit.sender_id
                .as_deref()
                .is_some_and(|sender_id| same_user(sender_id, &sender.user_id))
        });
        let date_holds = self.date.is_none_or(|range| {
            range.contains(hit.sent_at.with_timezone(&offset).date_naive(), today)
        });
        from_holds
            && date_holds
            && (!self.has_attachment || hit.has_attachment)
            && (!self.mentions_me || hit.mentions_me)
    }

    pub fn server_filters(&self, thread_id: &str) -> Vec<MessageFilter> {
        let mut filters = vec![MessageFilter::InChat {
            thread_id: thread_id.to_owned(),
        }];
        if let Some(sender) = &self.from {
            filters.push(MessageFilter::FromSender {
                mri: user_mri(&sender.user_id),
            });
        }
        if self.has_attachment {
            filters.push(MessageFilter::HasAttachment);
        }
        if self.mentions_me {
            filters.push(MessageFilter::MentionsMe);
        }
        filters
    }
}

fn bare_user_id(user_id: &str) -> String {
    user_id
        .strip_prefix(ORG_PREFIX)
        .unwrap_or(user_id)
        .to_ascii_lowercase()
}

pub fn same_user(first: &str, second: &str) -> bool {
    bare_user_id(first) == bare_user_id(second)
}

pub fn user_mri(user_id: &str) -> String {
    if user_id.contains(':') {
        user_id.to_owned()
    } else {
        format!("{ORG_PREFIX}{user_id}")
    }
}

fn parse_local_snippet(snippet: &str) -> (String, Vec<Range<usize>>) {
    let mut text = String::with_capacity(snippet.len());
    let mut highlights = Vec::new();
    let mut start = None;
    for character in snippet.chars() {
        match character {
            HIGHLIGHT_START => start = Some(text.len()),
            HIGHLIGHT_END => {
                if let Some(begin) = start.take() {
                    highlights.push(begin..text.len());
                }
            }
            _ => text.push(character),
        }
    }
    (text, highlights)
}

pub fn from_local(hit: &SearchHit, record: Option<&MessageRecord>, my_id: Option<&str>) -> FindHit {
    let (text, highlights) = parse_local_snippet(&hit.snippet);
    FindHit {
        message_id: hit.message_id.clone(),
        sent_at: hit.created_at,
        sender_name: hit.sender_name.clone().unwrap_or_default(),
        sender_id: record.and_then(|record| record.sender_id.clone()),
        text,
        highlights,
        mentions_me: record.is_some_and(|record| mentions_user(record, my_id)),
        has_attachment: record.is_some_and(has_attachment),
    }
}

pub fn from_record(record: &MessageRecord, my_id: Option<&str>) -> FindHit {
    FindHit {
        message_id: record.message_id.clone(),
        sent_at: record.created_at,
        sender_name: record.sender_name.clone().unwrap_or_default(),
        sender_id: record.sender_id.clone(),
        text: teams_core::preview_text(&record.body_html).unwrap_or_default(),
        highlights: Vec::new(),
        mentions_me: mentions_user(record, my_id),
        has_attachment: has_attachment(record),
    }
}

pub fn from_server(message: &SearchMessage) -> Option<FindHit> {
    let sent_at = message.sent_at?;
    let highlighted = split_highlights(
        message
            .highlighted_summary
            .as_deref()
            .unwrap_or(&message.preview),
    );
    Some(FindHit {
        message_id: message.message_id.clone(),
        sent_at,
        sender_name: message.sender_name.clone().unwrap_or_default(),
        sender_id: message.sender_mri.clone(),
        text: highlighted.text,
        highlights: highlighted.hits,
        mentions_me: message.is_mentioned,
        has_attachment: message.has_attachments,
    })
}

fn mentions_user(record: &MessageRecord, my_id: Option<&str>) -> bool {
    let Some(my_id) = my_id else {
        return false;
    };
    teams_core::mentions(record).iter().any(|mention| {
        mention
            .user_id
            .as_deref()
            .is_some_and(|user_id| same_user(user_id, my_id))
    })
}

fn has_attachment(record: &MessageRecord) -> bool {
    !teams_core::files(record).is_empty() || !teams_core::images(record).is_empty()
}

pub fn merge(local: Vec<FindHit>, server: Vec<FindHit>) -> Vec<FindHit> {
    let mut seen = HashSet::new();
    let mut hits: Vec<FindHit> = local
        .into_iter()
        .chain(server)
        .filter(|hit| seen.insert(hit.message_id.clone()))
        .collect();
    hits.sort_by(|first, second| {
        second
            .sent_at
            .cmp(&first.sent_at)
            .then_with(|| second.message_id.cmp(&first.message_id))
    });
    hits
}

pub fn month_label(date: NaiveDate, today: NaiveDate) -> String {
    let months = |date: NaiveDate| date.year() * 12 + date.month0() as i32;
    match months(today) - months(date) {
        0 => THIS_MONTH.to_owned(),
        1 => LAST_MONTH.to_owned(),
        _ => date.format("%B %Y").to_string(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonthGroup {
    pub label: String,
    pub hits: Vec<FindHit>,
}

pub fn group_by_month(hits: &[FindHit], today: NaiveDate, offset: FixedOffset) -> Vec<MonthGroup> {
    let mut groups: Vec<MonthGroup> = Vec::new();
    for hit in hits {
        let label = month_label(hit.sent_at.with_timezone(&offset).date_naive(), today);
        match groups.last_mut() {
            Some(group) if group.label == label => group.hits.push(hit.clone()),
            _ => groups.push(MonthGroup {
                label,
                hits: vec![hit.clone()],
            }),
        }
    }
    groups
}

pub fn step_index(current: Option<usize>, count: usize, backwards: bool) -> Option<usize> {
    if count == 0 {
        return None;
    }
    Some(match (current, backwards) {
        (None, false) => 0,
        (None, true) => count - 1,
        (Some(index), false) => (index + 1) % count,
        (Some(index), true) => (index + count - 1) % count,
    })
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn utc() -> FixedOffset {
        FixedOffset::east_opt(0).unwrap()
    }

    fn hit(message_id: &str, day: u32, month: u32) -> FindHit {
        FindHit {
            message_id: message_id.into(),
            sent_at: Utc.with_ymd_and_hms(2026, month, day, 10, 0, 0).unwrap(),
            sender_name: "Lea".into(),
            sender_id: Some("8:orgid:ABC".into()),
            text: message_id.into(),
            highlights: Vec::new(),
            mentions_me: false,
            has_attachment: false,
        }
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 17).unwrap()
    }

    #[test]
    fn merge_dedupes_by_message_id_and_sorts_newest_first() {
        let local = vec![hit("a", 1, 10), hit("b", 5, 10)];
        let mut server_copy = hit("a", 1, 10);
        server_copy.text = "server".into();
        let server = vec![server_copy, hit("c", 3, 10)];
        let merged = merge(local, server);
        let ids: Vec<&str> = merged.iter().map(|hit| hit.message_id.as_str()).collect();
        assert_eq!(ids, ["b", "c", "a"]);
        assert_eq!(merged[2].text, "a");
    }

    #[test]
    fn groups_follow_teams_month_labels() {
        let hits = vec![
            hit("a", 16, 10),
            hit("b", 2, 10),
            hit("c", 30, 9),
            hit("d", 1, 7),
        ];
        let groups = group_by_month(&hits, today(), utc());
        let labels: Vec<(&str, usize)> = groups
            .iter()
            .map(|group| (group.label.as_str(), group.hits.len()))
            .collect();
        assert_eq!(labels, [(THIS_MONTH, 2), (LAST_MONTH, 1), ("July 2026", 1)]);
        assert_eq!(
            month_label(NaiveDate::from_ymd_opt(2025, 10, 3).unwrap(), today()),
            "October 2025"
        );
    }

    #[test]
    fn date_ranges_use_the_local_calendar() {
        let saturday = today();
        let day = |day: u32| NaiveDate::from_ymd_opt(2026, 10, day).unwrap();
        assert!(DateRange::Today.contains(day(17), saturday));
        assert!(DateRange::Yesterday.contains(day(16), saturday));
        assert!(!DateRange::Yesterday.contains(day(17), saturday));
        assert!(DateRange::ThisWeek.contains(day(12), saturday));
        assert!(!DateRange::ThisWeek.contains(day(11), saturday));
        assert!(DateRange::ThisMonth.contains(day(1), saturday));
        assert!(
            !DateRange::ThisMonth.contains(NaiveDate::from_ymd_opt(2026, 9, 30).unwrap(), saturday)
        );
    }

    #[test]
    fn filters_admit_matching_hits_only() {
        let mut matching = hit("a", 17, 10);
        matching.has_attachment = true;
        matching.mentions_me = true;
        let plain = hit("b", 1, 10);
        let filters = FindFilters {
            from: Some(Sender {
                user_id: "abc".into(),
                name: "Lea".into(),
            }),
            date: Some(DateRange::Today),
            has_attachment: true,
            mentions_me: true,
        };
        assert!(filters.admits(&matching, today(), utc()));
        assert!(!filters.admits(&plain, today(), utc()));
        assert!(FindFilters::default().admits(&plain, today(), utc()));
        let other_sender = FindFilters {
            from: Some(Sender {
                user_id: "8:orgid:other".into(),
                name: "Other".into(),
            }),
            ..Default::default()
        };
        assert!(!other_sender.admits(&matching, today(), utc()));
    }

    #[test]
    fn server_filters_scope_to_the_chat_and_map_chips() {
        let filters = FindFilters {
            from: Some(Sender {
                user_id: "abc".into(),
                name: "Lea".into(),
            }),
            date: Some(DateRange::Today),
            has_attachment: true,
            mentions_me: true,
        };
        assert_eq!(
            filters.server_filters("19:chat@thread.v2"),
            [
                MessageFilter::InChat {
                    thread_id: "19:chat@thread.v2".into()
                },
                MessageFilter::FromSender {
                    mri: "8:orgid:abc".into()
                },
                MessageFilter::HasAttachment,
                MessageFilter::MentionsMe,
            ]
        );
    }

    #[test]
    fn local_snippet_markers_become_highlight_ranges() {
        let search_hit = SearchHit {
            conversation_id: "c".into(),
            message_id: "m".into(),
            sender_name: Some("Lea".into()),
            created_at: Utc.with_ymd_and_hms(2026, 10, 17, 9, 0, 0).unwrap(),
            snippet: format!("access is {HIGHLIGHT_START}ready{HIGHLIGHT_END} now"),
        };
        let found = from_local(&search_hit, None, None);
        assert_eq!(found.text, "access is ready now");
        assert_eq!(&found.text[found.highlights[0].clone()], "ready");
    }

    #[test]
    fn stepping_wraps_in_both_directions() {
        assert_eq!(step_index(None, 0, false), None);
        assert_eq!(step_index(None, 3, false), Some(0));
        assert_eq!(step_index(None, 3, true), Some(2));
        assert_eq!(step_index(Some(2), 3, false), Some(0));
        assert_eq!(step_index(Some(0), 3, true), Some(2));
    }
}
