use std::collections::HashMap;

use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::reaction_model::UNKNOWN_REACTOR;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    Mention,
    Reply,
    Reaction,
    MissedCall,
    Call,
    Posted,
    Team,
    App,
    #[default]
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    All,
    Mentions,
    Unread,
    Replies,
    Reactions,
    MissedCalls,
}

impl Filter {
    pub const PRIMARY: [Filter; 3] = [Filter::All, Filter::Mentions, Filter::Unread];
    pub const OVERFLOW: [Filter; 3] = [Filter::Replies, Filter::Reactions, Filter::MissedCalls];
    pub const ALL: [Filter; 6] = [
        Filter::All,
        Filter::Mentions,
        Filter::Unread,
        Filter::Replies,
        Filter::Reactions,
        Filter::MissedCalls,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Filter::All => "all",
            Filter::Mentions => "mentions",
            Filter::Unread => "unread",
            Filter::Replies => "replies",
            Filter::Reactions => "reactions",
            Filter::MissedCalls => "missed_calls",
        }
    }

    pub fn from_key(key: &str) -> Option<Filter> {
        Filter::ALL.into_iter().find(|filter| filter.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Mentions => "@Mentions",
            Filter::Unread => "Unread",
            Filter::Replies => "Replies",
            Filter::Reactions => "Reactions",
            Filter::MissedCalls => "Missed calls",
        }
    }

    pub fn empty_label(self) -> &'static str {
        match self {
            Filter::All => "No activity yet",
            Filter::Mentions => "No mentions",
            Filter::Unread => "Nothing unread",
            Filter::Replies => "No replies",
            Filter::Reactions => "No reactions",
            Filter::MissedCalls => "No missed calls",
        }
    }

    fn admits(self, entry: &Entry) -> bool {
        match self {
            Filter::All => true,
            Filter::Mentions => entry.kind == Kind::Mention,
            Filter::Unread => !entry.read,
            Filter::Replies => entry.kind == Kind::Reply,
            Filter::Reactions => entry.kind == Kind::Reaction,
            Filter::MissedCalls => entry.kind == Kind::MissedCall,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bucket {
    Today,
    Yesterday,
    Earlier,
}

impl Bucket {
    pub fn of(time: DateTime<Utc>, today: NaiveDate, offset: FixedOffset) -> Bucket {
        let days_ago = (today - time.with_timezone(&offset).date_naive()).num_days();
        match days_ago {
            ..=0 => Bucket::Today,
            1 => Bucket::Yesterday,
            _ => Bucket::Earlier,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Bucket::Today => "Today",
            Bucket::Yesterday => "Yesterday",
            Bucket::Earlier => "Earlier",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actor {
    pub user_id: Option<String>,
    pub name: String,
}

impl Actor {
    pub fn is_unresolved(&self) -> bool {
        self.user_id.is_some() && (self.name.is_empty() || self.name == UNKNOWN_REACTOR)
    }

    pub fn display_name<'a>(&'a self, resolved: &'a HashMap<String, String>) -> &'a str {
        if self.is_unresolved()
            && let Some(name) = self
                .user_id
                .as_ref()
                .and_then(|user_id| resolved.get(user_id))
        {
            return name;
        }
        &self.name
    }
}

/// One Activity row. `id` is the Teams activity message id in `48:notifications`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Entry {
    pub id: String,
    pub conversation_id: String,
    pub kind: Kind,
    pub activity_type: String,
    pub subtype: String,
    pub message_id: String,
    pub actors: Vec<Actor>,
    pub preview: String,
    pub glyphs: Vec<String>,
    pub count: u32,
    pub updated_at: DateTime<Utc>,
    pub read: bool,
    pub counts_for_bell: bool,
    pub source_label: Option<String>,
    pub link: Option<String>,
    pub reply_id: Option<String>,
    pub title: Option<String>,
    pub event_range: Option<(DateTime<Utc>, DateTime<Utc>)>,
}

impl Entry {
    pub fn unresolved_user_ids(&self) -> impl Iterator<Item = &String> {
        self.actors
            .iter()
            .filter(|actor| actor.is_unresolved())
            .filter_map(|actor| actor.user_id.as_ref())
    }

    pub fn latest_actor(&self) -> Option<&Actor> {
        self.actors.last()
    }

    pub fn is_personal_mention(&self) -> bool {
        self.kind == Kind::Mention && matches!(self.subtype.as_str(), "person" | "tag")
    }
}

pub struct Section<'a> {
    pub bucket: Bucket,
    pub entries: Vec<&'a Entry>,
}

#[derive(Default)]
pub struct Feed {
    entries: Vec<Entry>,
}

impl Feed {
    pub fn new(entries: Vec<Entry>) -> Feed {
        Feed { entries }
    }

    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn unread_count(&self) -> usize {
        self.unread_entries().count()
    }

    pub fn unread_entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|entry| !entry.read)
    }

    /// The Activity bell: unread rows newer than the last time the feed was opened.
    pub fn bell_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.counts_for_bell)
            .count()
    }

    pub fn sections(
        &self,
        filter: Filter,
        today: NaiveDate,
        offset: FixedOffset,
    ) -> Vec<Section<'_>> {
        let mut admitted: Vec<&Entry> = self
            .entries
            .iter()
            .filter(|entry| filter.admits(entry))
            .collect();
        admitted
            .sort_by(|left, right| (right.updated_at, &right.id).cmp(&(left.updated_at, &left.id)));
        let mut sections: Vec<Section<'_>> = Vec::new();
        for entry in admitted {
            let bucket = Bucket::of(entry.updated_at, today, offset);
            match sections.last_mut() {
                Some(section) if section.bucket == bucket => section.entries.push(entry),
                _ => sections.push(Section {
                    bucket,
                    entries: vec![entry],
                }),
            }
        }
        sections
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn at(day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, day, hour, 0, 0).unwrap()
    }

    fn entry(id: &str, kind: Kind, updated_at: DateTime<Utc>, read: bool) -> Entry {
        Entry {
            id: id.into(),
            kind,
            updated_at,
            read,
            counts_for_bell: !read,
            ..Entry::default()
        }
    }

    #[test]
    fn filters_round_trip_through_their_key() {
        for filter in Filter::ALL {
            assert_eq!(Filter::from_key(filter.key()), Some(filter));
        }
        assert_eq!(Filter::from_key("other"), None);
    }

    #[test]
    fn sections_group_newest_first_by_day_and_filter() {
        let feed = Feed::new(vec![
            entry("old", Kind::Reply, at(6, 9), true),
            entry("today", Kind::Mention, at(8, 9), false),
            entry("yesterday", Kind::Reaction, at(7, 9), false),
            entry("later", Kind::Mention, at(8, 10), true),
        ]);
        let today = at(8, 12).date_naive();
        let offset = FixedOffset::east_opt(0).unwrap();
        let sections = feed.sections(Filter::All, today, offset);
        let shape: Vec<(Bucket, Vec<&str>)> = sections
            .iter()
            .map(|section| {
                (
                    section.bucket,
                    section
                        .entries
                        .iter()
                        .map(|entry| entry.id.as_str())
                        .collect(),
                )
            })
            .collect();
        assert_eq!(
            shape,
            [
                (Bucket::Today, vec!["later", "today"]),
                (Bucket::Yesterday, vec!["yesterday"]),
                (Bucket::Earlier, vec!["old"]),
            ]
        );
        let mentions = feed.sections(Filter::Mentions, today, offset);
        assert_eq!(mentions[0].entries.len(), 2);
        let unread = feed.sections(Filter::Unread, today, offset);
        assert_eq!(
            unread
                .iter()
                .map(|section| section.entries.len())
                .sum::<usize>(),
            2
        );
        assert_eq!(feed.unread_count(), 2);
        assert_eq!(feed.bell_count(), 2);
    }

    #[test]
    fn kind_filters_keep_only_their_kind() {
        let feed = Feed::new(vec![
            entry("reply", Kind::Reply, at(8, 9), true),
            entry("reaction", Kind::Reaction, at(8, 9), false),
            entry("missed", Kind::MissedCall, at(8, 9), false),
            entry("call", Kind::Call, at(8, 9), false),
        ]);
        let today = at(8, 12).date_naive();
        let offset = FixedOffset::east_opt(0).unwrap();
        let ids = |filter| -> Vec<String> {
            feed.sections(filter, today, offset)
                .iter()
                .flat_map(|section| section.entries.iter().map(|entry| entry.id.clone()))
                .collect()
        };
        assert_eq!(ids(Filter::Replies), ["reply"]);
        assert_eq!(ids(Filter::Reactions), ["reaction"]);
        assert_eq!(ids(Filter::MissedCalls), ["missed"]);
    }
}
