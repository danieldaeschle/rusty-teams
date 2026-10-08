use std::collections::BTreeSet;

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use store::ActivityRecord;

use crate::notify::Incoming;
use crate::reaction_model::tooltip_text;

pub const RETENTION_DAYS: i64 = 14;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Messages,
    Mention,
    Reaction,
}

impl Kind {
    fn key(self) -> &'static str {
        match self {
            Kind::Messages => "messages",
            Kind::Mention => "mention",
            Kind::Reaction => "reaction",
        }
    }

    fn from_key(key: &str) -> Option<Kind> {
        [Kind::Messages, Kind::Mention, Kind::Reaction]
            .into_iter()
            .find(|kind| kind.key() == key)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    All,
    Mentions,
    Unread,
}

impl Filter {
    pub const ALL: [Filter; 3] = [Filter::All, Filter::Mentions, Filter::Unread];

    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Mentions => "@Mentions",
            Filter::Unread => "Unread",
        }
    }

    pub fn empty_label(self) -> &'static str {
        match self {
            Filter::All => "No activity yet",
            Filter::Mentions => "No mentions",
            Filter::Unread => "Nothing unread",
        }
    }

    fn admits(self, entry: &Entry) -> bool {
        match self {
            Filter::All => true,
            Filter::Mentions => entry.kind == Kind::Mention,
            Filter::Unread => !entry.read,
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

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub id: i64,
    pub conversation_id: String,
    pub kind: Kind,
    pub message_id: String,
    pub actors: Vec<Actor>,
    pub preview: String,
    pub glyphs: Vec<String>,
    pub count: u32,
    pub updated_at: DateTime<Utc>,
    pub read: bool,
}

impl Entry {
    pub fn names(&self) -> String {
        let names: Vec<&str> = self
            .actors
            .iter()
            .map(|actor| actor.name.as_str())
            .collect();
        tooltip_text(&names)
    }

    pub fn latest_actor(&self) -> Option<&Actor> {
        self.actors.last()
    }

    pub fn to_record(&self) -> ActivityRecord {
        ActivityRecord {
            id: self.id,
            conversation_id: self.conversation_id.clone(),
            kind: self.kind.key().to_owned(),
            message_id: self.message_id.clone(),
            actors_json: serde_json::to_string(&self.actors).unwrap_or_else(|_| "[]".to_owned()),
            preview: self.preview.clone(),
            glyphs: serde_json::to_string(&self.glyphs).unwrap_or_else(|_| "[]".to_owned()),
            count: self.count,
            updated_at: self.updated_at,
            read: self.read,
        }
    }

    pub fn from_record(record: ActivityRecord) -> Option<Entry> {
        Some(Entry {
            id: record.id,
            kind: Kind::from_key(&record.kind)?,
            conversation_id: record.conversation_id,
            message_id: record.message_id,
            actors: serde_json::from_str(&record.actors_json).unwrap_or_default(),
            preview: record.preview,
            glyphs: serde_json::from_str(&record.glyphs).unwrap_or_default(),
            count: record.count,
            updated_at: record.updated_at,
            read: record.read,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sighting {
    pub user_id: Option<String>,
    pub name: String,
    pub glyph: String,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReactedMessage {
    pub message_id: String,
    pub preview: String,
    pub sightings: Vec<Sighting>,
}

pub struct Section<'a> {
    pub bucket: Bucket,
    pub entries: Vec<&'a Entry>,
}

#[derive(Default)]
pub struct Feed {
    entries: Vec<Entry>,
    next_id: i64,
    dirty: BTreeSet<i64>,
}

pub fn start_time(saved: Option<DateTime<Utc>>, now: DateTime<Utc>) -> DateTime<Utc> {
    saved
        .unwrap_or(now)
        .clamp(now - Duration::days(RETENTION_DAYS), now)
}

impl Feed {
    pub fn load(records: Vec<ActivityRecord>) -> Feed {
        let entries: Vec<Entry> = records.into_iter().filter_map(Entry::from_record).collect();
        let next_id = entries.iter().map(|entry| entry.id).max().unwrap_or(0) + 1;
        Feed {
            entries,
            next_id,
            dirty: BTreeSet::new(),
        }
    }

    pub fn add(&mut self, mut entry: Entry) -> i64 {
        entry.id = self.next_id;
        self.next_id += 1;
        self.dirty.insert(entry.id);
        let id = entry.id;
        self.entries.push(entry);
        id
    }

    pub fn get(&self, id: i64) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    pub fn unread_count(&self) -> usize {
        self.entries.iter().filter(|entry| !entry.read).count()
    }

    pub fn take_dirty(&mut self) -> Vec<Entry> {
        let dirty = std::mem::take(&mut self.dirty);
        self.entries
            .iter()
            .filter(|entry| dirty.contains(&entry.id))
            .cloned()
            .collect()
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
        admitted.sort_by_key(|entry| std::cmp::Reverse((entry.updated_at, entry.id)));
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

    pub fn record_message(&mut self, incoming: &Incoming, preview: String) {
        let actor = Actor {
            user_id: incoming.sender_id.clone(),
            name: incoming.sender_name.clone(),
        };
        if incoming.mentions_me {
            self.add(Entry {
                id: 0,
                conversation_id: incoming.conversation_id.clone(),
                kind: Kind::Mention,
                message_id: incoming.message_id.clone(),
                actors: vec![actor],
                preview,
                glyphs: Vec::new(),
                count: 1,
                updated_at: incoming.created_at,
                read: false,
            });
            return;
        }
        let open_row = self.entries.iter().position(|entry| {
            entry.kind == Kind::Messages
                && !entry.read
                && entry.conversation_id == incoming.conversation_id
        });
        let Some(index) = open_row else {
            self.add(Entry {
                id: 0,
                conversation_id: incoming.conversation_id.clone(),
                kind: Kind::Messages,
                message_id: incoming.message_id.clone(),
                actors: vec![actor],
                preview,
                glyphs: Vec::new(),
                count: 1,
                updated_at: incoming.created_at,
                read: false,
            });
            return;
        };
        let entry = &mut self.entries[index];
        entry.count += 1;
        entry.preview = preview;
        entry.updated_at = incoming.created_at;
        move_to_end(&mut entry.actors, actor);
        self.dirty.insert(entry.id);
    }

    pub fn record_chat_preview(
        &mut self,
        conversation_id: &str,
        actor: Actor,
        preview: String,
        at: DateTime<Utc>,
    ) {
        let known = self.entries.iter().any(|entry| {
            entry.conversation_id == conversation_id
                && entry.kind != Kind::Reaction
                && entry.updated_at >= at
        });
        if known {
            return;
        }
        let open_row = self.entries.iter().position(|entry| {
            entry.kind == Kind::Messages && !entry.read && entry.conversation_id == conversation_id
        });
        match open_row {
            Some(index) => {
                let entry = &mut self.entries[index];
                entry.count += 1;
                entry.preview = preview;
                entry.updated_at = at;
                move_to_end(&mut entry.actors, actor);
                self.dirty.insert(entry.id);
            }
            None => {
                self.add(Entry {
                    id: 0,
                    conversation_id: conversation_id.to_owned(),
                    kind: Kind::Messages,
                    message_id: String::new(),
                    actors: vec![actor],
                    preview,
                    glyphs: Vec::new(),
                    count: 1,
                    updated_at: at,
                    read: false,
                });
            }
        }
    }

    pub fn record_reactions(
        &mut self,
        conversation_id: &str,
        message: &ReactedMessage,
        my_user_id: &str,
        cutoff: DateTime<Utc>,
        now: DateTime<Utc>,
    ) {
        for sighting in &message.sightings {
            let Some(user_id) = sighting.user_id.as_deref().filter(|id| *id != my_user_id) else {
                continue;
            };
            let existing = self.entries.iter().position(|entry| {
                entry.kind == Kind::Reaction
                    && entry.conversation_id == conversation_id
                    && entry.message_id == message.message_id
            });
            let known = existing.is_some_and(|index| {
                self.entries[index]
                    .actors
                    .iter()
                    .any(|actor| actor.user_id.as_deref() == Some(user_id))
            });
            if known {
                if let Some(index) = existing {
                    self.add_glyph(index, &sighting.glyph);
                }
                continue;
            }
            let fresh = match sighting.created_at {
                Some(created_at) => created_at > cutoff,
                None => existing.is_some(),
            };
            if !fresh {
                continue;
            }
            let actor = Actor {
                user_id: Some(user_id.to_owned()),
                name: sighting.name.clone(),
            };
            let at = sighting.created_at.unwrap_or(now);
            match existing {
                Some(index) => {
                    let entry = &mut self.entries[index];
                    entry.actors.push(actor);
                    entry.count = entry.actors.len() as u32;
                    entry.read = false;
                    entry.updated_at = entry.updated_at.max(at);
                    self.dirty.insert(entry.id);
                    self.add_glyph(index, &sighting.glyph);
                }
                None => {
                    self.add(Entry {
                        id: 0,
                        conversation_id: conversation_id.to_owned(),
                        kind: Kind::Reaction,
                        message_id: message.message_id.clone(),
                        actors: vec![actor],
                        preview: message.preview.clone(),
                        glyphs: vec![sighting.glyph.clone()],
                        count: 1,
                        updated_at: at,
                        read: false,
                    });
                }
            }
        }
    }

    fn add_glyph(&mut self, index: usize, glyph: &str) {
        let entry = &mut self.entries[index];
        if !glyph.is_empty() && !entry.glyphs.iter().any(|known| known == glyph) {
            entry.glyphs.push(glyph.to_owned());
            self.dirty.insert(entry.id);
        }
    }

    pub fn mark_read(&mut self, id: i64) -> bool {
        self.mark_where(|entry| entry.id == id)
    }

    pub fn mark_all_read(&mut self) -> bool {
        self.mark_where(|_| true)
    }

    pub fn mark_conversation_read(&mut self, conversation_id: &str) -> bool {
        self.mark_where(|entry| entry.conversation_id == conversation_id)
    }

    pub fn sync_read(&mut self, read_at: impl Fn(&str) -> Option<DateTime<Utc>>) -> bool {
        self.mark_where(|entry| {
            read_at(&entry.conversation_id).is_some_and(|read_at| entry.updated_at <= read_at)
        })
    }

    fn mark_where(&mut self, matches: impl Fn(&Entry) -> bool) -> bool {
        let mut changed = false;
        for entry in self.entries.iter_mut().filter(|entry| !entry.read) {
            if matches(entry) {
                entry.read = true;
                self.dirty.insert(entry.id);
                changed = true;
            }
        }
        changed
    }
}

fn move_to_end(actors: &mut Vec<Actor>, actor: Actor) {
    actors.retain(|known| match (&known.user_id, &actor.user_id) {
        (Some(known_id), Some(id)) => known_id != id,
        _ => known.name != actor.name,
    });
    actors.push(actor);
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;
    use crate::notify::{ChatKind, Preview};

    fn at(hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 8, hour, minute, 0).unwrap()
    }

    fn incoming(conversation_id: &str, message_id: &str, sender: &str, minute: u32) -> Incoming {
        Incoming {
            conversation_id: conversation_id.into(),
            message_id: message_id.into(),
            kind: ChatKind::Direct,
            chat_title: "Chat".into(),
            sender_id: Some(format!("id-{sender}")),
            sender_name: sender.into(),
            preview: Preview::Text(format!("text {message_id}")),
            mentions_me: false,
            muted: false,
            signals: Default::default(),
            created_at: at(9, minute),
        }
    }

    fn record(feed: &mut Feed, incoming: &Incoming) {
        feed.record_message(incoming, format!("text {}", incoming.message_id));
    }

    fn sighting(user: &str, glyph: &str, created_at: Option<DateTime<Utc>>) -> Sighting {
        Sighting {
            user_id: Some(format!("id-{user}")),
            name: user.into(),
            glyph: glyph.into(),
            created_at,
        }
    }

    fn react(feed: &mut Feed, sightings: &[Sighting]) {
        let message = ReactedMessage {
            message_id: "mine".into(),
            preview: "my text".into(),
            sightings: sightings.to_vec(),
        };
        feed.record_reactions("chat", &message, "id-me", at(8, 0), at(10, 0));
    }

    fn all(feed: &Feed) -> Vec<&Entry> {
        feed.sections(Filter::All, at(9, 0).date_naive(), offset())
            .into_iter()
            .flat_map(|section| section.entries)
            .collect()
    }

    fn offset() -> FixedOffset {
        FixedOffset::east_opt(0).unwrap()
    }

    #[test]
    fn three_messages_share_one_row_and_keep_the_first_message_id() {
        let mut feed = Feed::default();
        for (index, message_id) in ["m1", "m2", "m3"].into_iter().enumerate() {
            record(
                &mut feed,
                &incoming("chat", message_id, "Anna", index as u32),
            );
        }
        let entries = all(&feed);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].count, 3);
        assert_eq!(entries[0].message_id, "m1");
        assert_eq!(entries[0].preview, "text m3");
        assert_eq!(entries[0].updated_at, at(9, 2));
        assert_eq!(feed.unread_count(), 1);
    }

    #[test]
    fn latest_sender_moves_to_the_end() {
        let mut feed = Feed::default();
        record(&mut feed, &incoming("chat", "m1", "Anna", 1));
        record(&mut feed, &incoming("chat", "m2", "Ben", 2));
        record(&mut feed, &incoming("chat", "m3", "Anna", 3));
        let entries = all(&feed);
        let entry = entries[0];
        assert_eq!(entry.names(), "Ben and Anna");
        assert_eq!(entry.latest_actor().unwrap().name, "Anna");
    }

    #[test]
    fn chat_preview_adds_a_row_only_when_newer_than_the_feed() {
        let mut feed = Feed::default();
        record(&mut feed, &incoming("chat", "m1", "Anna", 1));
        let anna = Actor {
            user_id: Some("id-Anna".into()),
            name: "Anna".into(),
        };
        feed.record_chat_preview("chat", anna.clone(), "text m1".into(), at(9, 1));
        assert_eq!(all(&feed)[0].count, 1);
        feed.record_chat_preview("chat", anna.clone(), "later".into(), at(9, 5));
        assert_eq!(all(&feed)[0].count, 2);
        assert_eq!(all(&feed)[0].preview, "later");
        feed.record_chat_preview("other", anna, "offline".into(), at(9, 6));
        let entries = all(&feed);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].message_id, "");
    }

    #[test]
    fn message_after_read_opens_a_new_row() {
        let mut feed = Feed::default();
        record(&mut feed, &incoming("chat", "m1", "Anna", 1));
        feed.mark_conversation_read("chat");
        record(&mut feed, &incoming("chat", "m2", "Anna", 2));
        let entries = all(&feed);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].message_id, "m2");
        assert!(!entries[0].read);
        assert!(entries[1].read);
        assert_eq!(entries[1].count, 1);
    }

    #[test]
    fn mention_gets_its_own_row_and_is_not_counted_in_messages() {
        let mut feed = Feed::default();
        record(&mut feed, &incoming("chat", "m1", "Anna", 1));
        let mut mention = incoming("chat", "m2", "Ben", 2);
        mention.mentions_me = true;
        record(&mut feed, &mention);
        record(&mut feed, &incoming("chat", "m3", "Anna", 3));
        let entries = all(&feed);
        assert_eq!(entries.len(), 2);
        let mention_row = entries
            .iter()
            .find(|entry| entry.kind == Kind::Mention)
            .unwrap();
        assert_eq!(mention_row.message_id, "m2");
        let messages_row = entries
            .iter()
            .find(|entry| entry.kind == Kind::Messages)
            .unwrap();
        assert_eq!(messages_row.count, 2);
        assert_eq!(messages_row.message_id, "m1");
    }

    #[test]
    fn reactions_bundle_per_message_and_become_unread_again() {
        let mut feed = Feed::default();
        react(&mut feed, &[sighting("Anna", "A", Some(at(9, 1)))]);
        feed.mark_all_read();
        assert_eq!(feed.unread_count(), 0);
        react(
            &mut feed,
            &[
                sighting("Anna", "A", Some(at(9, 1))),
                sighting("Ben", "B", Some(at(9, 5))),
            ],
        );
        let entries = all(&feed);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].names(), "Anna and Ben");
        assert_eq!(entries[0].glyphs, vec!["A", "B"]);
        assert_eq!(entries[0].count, 2);
        assert_eq!(entries[0].updated_at, at(9, 5));
        assert!(!entries[0].read);
        assert_eq!(entries[0].preview, "my text");
    }

    #[test]
    fn known_reactor_adding_a_glyph_does_not_reopen_the_row() {
        let mut feed = Feed::default();
        react(&mut feed, &[sighting("Anna", "A", Some(at(9, 1)))]);
        feed.mark_all_read();
        react(&mut feed, &[sighting("Anna", "C", Some(at(9, 2)))]);
        let entries = all(&feed);
        assert_eq!(entries[0].glyphs, vec!["A", "C"]);
        assert!(entries[0].read);
    }

    #[test]
    fn own_and_anonymous_reactions_are_ignored() {
        let mut feed = Feed::default();
        let anonymous = Sighting {
            user_id: None,
            ..sighting("Anna", "A", Some(at(9, 1)))
        };
        react(&mut feed, &[sighting("me", "A", Some(at(9, 1))), anonymous]);
        assert!(all(&feed).is_empty());
    }

    #[test]
    fn old_reactions_on_first_sight_are_ignored() {
        let mut feed = Feed::default();
        react(
            &mut feed,
            &[
                sighting("Anna", "A", Some(at(7, 0))),
                sighting("Ben", "B", None),
            ],
        );
        assert!(all(&feed).is_empty());
        react(&mut feed, &[sighting("Cleo", "C", Some(at(9, 0)))]);
        react(&mut feed, &[sighting("Dan", "D", None)]);
        assert_eq!(all(&feed)[0].names(), "Cleo and Dan");
    }

    #[test]
    fn reading_a_conversation_leaves_others_unread() {
        let mut feed = Feed::default();
        record(&mut feed, &incoming("a", "m1", "Anna", 1));
        record(&mut feed, &incoming("b", "m2", "Ben", 2));
        assert!(feed.mark_conversation_read("a"));
        assert!(!feed.mark_conversation_read("a"));
        assert_eq!(feed.unread_count(), 1);
        let unread = all(&feed).into_iter().find(|entry| !entry.read).unwrap();
        assert_eq!(unread.conversation_id, "b");
    }

    #[test]
    fn read_sync_only_covers_rows_up_to_the_read_time() {
        let mut feed = Feed::default();
        record(&mut feed, &incoming("a", "m1", "Anna", 1));
        feed.mark_conversation_read("a");
        record(&mut feed, &incoming("a", "m2", "Anna", 30));
        record(&mut feed, &incoming("b", "m3", "Ben", 1));
        let changed = feed.sync_read(|conversation_id| (conversation_id == "a").then(|| at(9, 10)));
        assert!(!changed);
        let changed = feed.sync_read(|conversation_id| (conversation_id == "a").then(|| at(9, 45)));
        assert!(changed);
        assert_eq!(feed.unread_count(), 1);
    }

    #[test]
    fn mark_all_read_clears_the_count_and_reports_change_once() {
        let mut feed = Feed::default();
        record(&mut feed, &incoming("a", "m1", "Anna", 1));
        record(&mut feed, &incoming("b", "m2", "Ben", 2));
        assert!(feed.mark_all_read());
        assert_eq!(feed.unread_count(), 0);
        assert!(!feed.mark_all_read());
    }

    #[test]
    fn filters_pick_mentions_and_unread() {
        let mut feed = Feed::default();
        record(&mut feed, &incoming("a", "m1", "Anna", 1));
        let mut mention = incoming("b", "m2", "Ben", 2);
        mention.mentions_me = true;
        record(&mut feed, &mention);
        feed.mark_conversation_read("a");
        let today = at(9, 0).date_naive();
        let count = |filter| {
            feed.sections(filter, today, offset())
                .iter()
                .map(|section| section.entries.len())
                .sum::<usize>()
        };
        assert_eq!(count(Filter::All), 2);
        assert_eq!(count(Filter::Mentions), 1);
        assert_eq!(count(Filter::Unread), 1);
    }

    #[test]
    fn buckets_follow_the_local_day() {
        let today = at(12, 0).date_naive();
        let utc = offset();
        assert_eq!(Bucket::of(at(0, 5), today, utc), Bucket::Today);
        assert_eq!(
            Bucket::of(at(0, 5) - Duration::minutes(10), today, utc),
            Bucket::Yesterday
        );
        assert_eq!(
            Bucket::of(at(0, 5) - Duration::days(3), today, utc),
            Bucket::Earlier
        );
        let east = FixedOffset::east_opt(2 * 3600).unwrap();
        assert_eq!(
            Bucket::of(at(23, 30) - Duration::days(1), today, east),
            Bucket::Today
        );
    }

    #[test]
    fn sections_are_newest_first_and_grouped() {
        let mut feed = Feed::default();
        let mut old = incoming("a", "m1", "Anna", 1);
        old.created_at = at(9, 0) - Duration::days(1);
        record(&mut feed, &old);
        record(&mut feed, &incoming("b", "m2", "Ben", 2));
        record(&mut feed, &incoming("c", "m3", "Cleo", 3));
        let today = at(9, 0).date_naive();
        let sections = feed.sections(Filter::All, today, offset());
        let labels: Vec<&str> = sections
            .iter()
            .map(|section| section.bucket.label())
            .collect();
        assert_eq!(labels, ["Today", "Yesterday"]);
        assert_eq!(sections[0].entries[0].conversation_id, "c");
    }

    #[test]
    fn dirty_rows_are_reported_once_and_survive_a_round_trip() {
        let mut feed = Feed::default();
        record(&mut feed, &incoming("a", "m1", "Anna", 1));
        react(&mut feed, &[sighting("Ben", "B", Some(at(9, 1)))]);
        let dirty = feed.take_dirty();
        assert_eq!(dirty.len(), 2);
        assert!(feed.take_dirty().is_empty());
        let reloaded = Feed::load(dirty.iter().map(Entry::to_record).collect());
        assert_eq!(all(&reloaded).len(), 2);
        let mut reloaded = reloaded;
        record(&mut reloaded, &incoming("c", "m9", "Cleo", 9));
        let ids: BTreeSet<i64> = reloaded.take_dirty().iter().map(|entry| entry.id).collect();
        assert_eq!(ids.len(), 1);
        assert!(!dirty.iter().any(|entry| ids.contains(&entry.id)));
    }

    #[test]
    fn start_time_defaults_to_now_and_is_clamped() {
        let now = at(12, 0);
        assert_eq!(start_time(None, now), now);
        assert_eq!(start_time(Some(now + Duration::hours(1)), now), now);
        assert_eq!(
            start_time(Some(now - Duration::days(30)), now),
            now - Duration::days(RETENTION_DAYS)
        );
        assert_eq!(start_time(Some(at(8, 0)), now), at(8, 0));
    }
}
