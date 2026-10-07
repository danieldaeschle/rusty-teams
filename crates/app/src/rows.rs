use std::collections::BTreeMap;
use std::ops::Range;

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, Utc};
use store::MessageRecord;
use teams_core::{FileCard, ImageRef, ReactionInfo, Span, files, images, message_spans, reactions};

use crate::format;
use crate::render::blocks::{Inline, MENTION_PAD, strip_image_placeholders};
use crate::render::{Block, layout_blocks};
use crate::sidebar_model::Face;

const REPLY_FACE_LIMIT: usize = 3;

pub const LOAD_OLDER_KEY: &str = "load-older";
const UNKNOWN_AUTHOR: &str = "Unknown";
const DELETED_TEXT: &str = "Message deleted";
const SERIES_GAP_MINUTES: i64 = 5;
const CORNER_RADIUS: f32 = 14.;
const CORNER_SERIES: f32 = 4.;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReactionChip {
    pub label: String,
    pub count: usize,
    pub mine: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Series {
    pub has_prev: bool,
    pub has_next: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Corners {
    pub top_left: f32,
    pub top_right: f32,
    pub bottom_right: f32,
    pub bottom_left: f32,
}

pub fn bubble_corners(own: bool, series: Series) -> Corners {
    if own {
        let top_right = if series.has_prev || !series.has_next {
            CORNER_SERIES
        } else {
            CORNER_RADIUS
        };
        let bottom_right = if series.has_next || !series.has_prev {
            CORNER_SERIES
        } else {
            CORNER_RADIUS
        };
        Corners {
            top_left: CORNER_RADIUS,
            top_right,
            bottom_right,
            bottom_left: CORNER_RADIUS,
        }
    } else {
        Corners {
            top_left: CORNER_SERIES,
            top_right: CORNER_RADIUS,
            bottom_right: CORNER_RADIUS,
            bottom_left: if series.has_next {
                CORNER_SERIES
            } else {
                CORNER_RADIUS
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Receipt {
    #[default]
    Hidden,
    Pending,
    Sent,
    Read,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
    Delivered,
    Sending,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MessageRow {
    pub key: String,
    pub author: String,
    pub sender_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub series: Series,
    pub card: bool,
    pub time: String,
    pub day_header: Option<String>,
    pub blocks: Vec<Block>,
    pub edited: bool,
    pub deleted: bool,
    pub reactions: Vec<ReactionChip>,
    pub images: Vec<ImageRef>,
    pub files: Vec<FileCard>,
    pub reply_count: Option<usize>,
    pub new_marker: bool,
    pub reply_faces: Vec<Face>,
    pub last_reply_time: Option<String>,
    pub open_thread: Option<String>,
    pub is_reply: bool,
    pub delivery: Delivery,
    pub own: bool,
    pub receipt: Receipt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartInfo {
    pub title: String,
    pub user_id: Option<String>,
}

pub const START_KEY: &str = "conversation-start";

#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    LoadOlder,
    Start(StartInfo),
    Skeleton(Skeleton),
    Message(Box<MessageRow>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Skeleton {
    pub key: &'static str,
    pub own: bool,
    pub width_ratio: f32,
    pub lines: u8,
}

const SKELETON_TRAILING: Skeleton = Skeleton {
    key: "skeleton-trailing",
    own: false,
    width_ratio: 0.32,
    lines: 1,
};

const SKELETON_PLACEHOLDERS: [Skeleton; 4] = [
    Skeleton {
        key: "skeleton-0",
        own: false,
        width_ratio: 0.42,
        lines: 1,
    },
    Skeleton {
        key: "skeleton-1",
        own: true,
        width_ratio: 0.3,
        lines: 1,
    },
    Skeleton {
        key: "skeleton-2",
        own: true,
        width_ratio: 0.48,
        lines: 2,
    },
    Skeleton {
        key: "skeleton-3",
        own: false,
        width_ratio: 0.26,
        lines: 1,
    },
];

pub fn placeholder_rows() -> Vec<Row> {
    SKELETON_PLACEHOLDERS
        .into_iter()
        .map(Row::Skeleton)
        .collect()
}

pub fn trailing_skeleton() -> Row {
    Row::Skeleton(SKELETON_TRAILING)
}

fn sender_key(row: &MessageRow) -> &str {
    row.sender_id.as_deref().unwrap_or(&row.author)
}

fn continues_series(previous: &MessageRow, next: &MessageRow) -> bool {
    !previous.card
        && !next.card
        && next.day_header.is_none()
        && !next.new_marker
        && sender_key(previous) == sender_key(next)
        && next.created_at - previous.created_at < Duration::minutes(SERIES_GAP_MINUTES)
        && next.created_at >= previous.created_at
}

pub fn assign_series(rows: &mut [Row]) {
    let mut previous: Option<usize> = None;
    for index in 0..rows.len() {
        let Row::Message(_) = &rows[index] else {
            previous = None;
            continue;
        };
        if let Some(before) = previous {
            let joined = match (&rows[before], &rows[index]) {
                (Row::Message(a), Row::Message(b)) => continues_series(a, b),
                _ => false,
            };
            if joined {
                if let Row::Message(a) = &mut rows[before] {
                    a.series.has_next = true;
                }
                if let Row::Message(b) = &mut rows[index] {
                    b.series.has_prev = true;
                }
            }
        }
        previous = Some(index);
    }
}

impl Row {
    pub fn key(&self) -> &str {
        match self {
            Row::LoadOlder => LOAD_OLDER_KEY,
            Row::Start(_) => START_KEY,
            Row::Skeleton(skeleton) => skeleton.key,
            Row::Message(message) => &message.key,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    pub root_id: String,
    pub reply_ids: Vec<String>,
}

pub fn clock_label(time: DateTime<Utc>, offset: FixedOffset) -> String {
    time.with_timezone(&offset).format("%H:%M").to_string()
}

pub fn reaction_label(reaction_type: &str) -> String {
    match reaction_type {
        "like" => "\u{1F44D}".to_owned(),
        "heart" => "\u{2764}".to_owned(),
        "laugh" => "\u{1F602}".to_owned(),
        "surprised" => "\u{1F62E}".to_owned(),
        "sad" => "\u{1F622}".to_owned(),
        "angry" => "\u{1F620}".to_owned(),
        other => other.to_owned(),
    }
}

pub fn reaction_chips(reactions: &[ReactionInfo], my_user_id: Option<&str>) -> Vec<ReactionChip> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut mine: BTreeMap<&str, bool> = BTreeMap::new();
    let mut order: Vec<&str> = Vec::new();
    for reaction in reactions {
        let entry = counts.entry(reaction.reaction_type.as_str()).or_insert(0);
        if *entry == 0 {
            order.push(reaction.reaction_type.as_str());
        }
        *entry += 1;
        if my_user_id.is_some() && reaction.user_id.as_deref() == my_user_id {
            mine.insert(reaction.reaction_type.as_str(), true);
        }
    }
    order
        .into_iter()
        .map(|reaction_type| ReactionChip {
            label: reaction_label(reaction_type),
            count: counts[reaction_type],
            mine: mine.contains_key(reaction_type),
        })
        .collect()
}

const REPLY_EXCERPT_CHARS: usize = 90;

pub fn reply_excerpt(record: &MessageRecord) -> String {
    let own_spans: Vec<Span> = message_spans(record)
        .into_iter()
        .filter(|span| !matches!(span, Span::Quote(_)))
        .collect();
    let text = strip_image_placeholders(layout_blocks(&own_spans))
        .into_iter()
        .find_map(|block| match block {
            Block::Paragraph(inline) | Block::ListItem(inline) => Some(inline.text),
            Block::Code(code) => Some(code),
            Block::Quote(_) => None,
        })
        .unwrap_or_default()
        .replace(MENTION_PAD, "");
    let first_line = text.lines().map(str::trim).find(|line| !line.is_empty());
    let excerpt = match first_line {
        Some(line) => line.to_owned(),
        None if !images(record).is_empty() => "Image".to_owned(),
        None => files(record)
            .into_iter()
            .next()
            .map_or_else(|| "Message".to_owned(), |card| card.name),
    };
    if excerpt.chars().count() > REPLY_EXCERPT_CHARS {
        let cut: String = excerpt.chars().take(REPLY_EXCERPT_CHARS).collect();
        format!("{}...", cut.trim_end())
    } else {
        excerpt
    }
}

pub struct RowContext {
    pub offset: FixedOffset,
    pub today: NaiveDate,
    pub my_user_id: Option<String>,
}

pub fn message_row(record: &MessageRecord, context: &RowContext) -> MessageRow {
    let local = record.created_at.with_timezone(&context.offset);
    let images = images(record);
    let blocks = if record.deleted {
        vec![Block::Paragraph(Inline::plain(DELETED_TEXT))]
    } else {
        layout_blocks(&message_spans(record))
    };
    let blocks = if images.is_empty() {
        blocks
    } else {
        strip_image_placeholders(blocks)
    };
    MessageRow {
        key: record.message_id.clone(),
        sender_id: record.sender_id.clone(),
        created_at: record.created_at,
        series: Series::default(),
        card: false,
        author: record
            .sender_name
            .clone()
            .unwrap_or_else(|| UNKNOWN_AUTHOR.to_owned()),
        time: clock_label(record.created_at, context.offset),
        day_header: Some(format::day_label(local.date_naive(), context.today)),
        blocks,
        edited: record.edited_at.is_some() && !record.deleted,
        deleted: record.deleted,
        reactions: reaction_chips(&reactions(record), context.my_user_id.as_deref()),
        images,
        files: files(record),
        reply_count: None,
        new_marker: false,
        reply_faces: Vec::new(),
        last_reply_time: None,
        open_thread: None,
        is_reply: record.reply_to_id.is_some(),
        delivery: Delivery::Delivered,
        receipt: Receipt::Hidden,
        own: context.my_user_id.is_some() && record.sender_id == context.my_user_id,
    }
}

pub fn flat_rows(records: &[MessageRecord], context: &RowContext, has_older: bool) -> Vec<Row> {
    let mut rows = Vec::with_capacity(records.len() + 1);
    if has_older {
        rows.push(Row::LoadOlder);
    }
    let mut previous_day: Option<NaiveDate> = None;
    for record in records {
        let mut row = message_row(record, context);
        let day = record
            .created_at
            .with_timezone(&context.offset)
            .date_naive();
        if previous_day == Some(day) {
            row.day_header = None;
        }
        previous_day = Some(day);
        rows.push(Row::Message(Box::new(row)));
    }
    assign_series(&mut rows);
    rows
}

pub fn group_threads(records: &[MessageRecord]) -> Vec<Thread> {
    let mut threads: Vec<Thread> = Vec::new();
    let mut index_by_root: BTreeMap<&str, usize> = BTreeMap::new();
    for record in records.iter().filter(|record| record.reply_to_id.is_none()) {
        index_by_root.insert(record.message_id.as_str(), threads.len());
        threads.push(Thread {
            root_id: record.message_id.clone(),
            reply_ids: Vec::new(),
        });
    }
    for record in records {
        let Some(root_id) = record.reply_to_id.as_deref() else {
            continue;
        };
        if let Some(&index) = index_by_root.get(root_id) {
            threads[index].reply_ids.push(record.message_id.clone());
        }
    }
    threads
}

fn last_activity(thread: &Thread, by_id: &BTreeMap<&str, &MessageRecord>) -> DateTime<Utc> {
    thread
        .reply_ids
        .iter()
        .chain(std::iter::once(&thread.root_id))
        .filter_map(|id| by_id.get(id.as_str()))
        .map(|record| record.created_at)
        .max()
        .unwrap_or_default()
}

pub fn thread_list_rows(
    records: &[MessageRecord],
    context: &RowContext,
    has_older: bool,
) -> Vec<Row> {
    let by_id: BTreeMap<&str, &MessageRecord> = records
        .iter()
        .map(|record| (record.message_id.as_str(), record))
        .collect();
    let mut threads = group_threads(records);
    threads.sort_by_key(|thread| last_activity(thread, &by_id));
    let mut rows = Vec::with_capacity(threads.len() + 1);
    if has_older {
        rows.push(Row::LoadOlder);
    }
    let mut previous_day: Option<NaiveDate> = None;
    for thread in threads {
        let Some(root) = by_id.get(thread.root_id.as_str()) else {
            continue;
        };
        let mut row = message_row(root, context);
        let day = last_activity(&thread, &by_id)
            .with_timezone(&context.offset)
            .date_naive();
        row.day_header = (previous_day != Some(day)).then(|| format::day_label(day, context.today));
        previous_day = Some(day);
        row.card = true;
        row.reply_count = Some(thread.reply_ids.len());
        let replies: Vec<&&MessageRecord> = thread
            .reply_ids
            .iter()
            .filter_map(|id| by_id.get(id.as_str()))
            .collect();
        for reply in &replies {
            let face = Face {
                user_id: reply.sender_id.clone(),
                name: reply
                    .sender_name
                    .clone()
                    .unwrap_or_else(|| UNKNOWN_AUTHOR.to_owned()),
            };
            if row.reply_faces.len() < REPLY_FACE_LIMIT && !row.reply_faces.contains(&face) {
                row.reply_faces.push(face);
            }
        }
        row.last_reply_time = replies
            .iter()
            .map(|reply| reply.created_at)
            .max()
            .map(|time| {
                let local = time.with_timezone(&context.offset);
                format!(
                    "{} {}",
                    format::day_label(local.date_naive(), context.today),
                    clock_label(time, context.offset)
                )
            });
        row.open_thread = Some(thread.root_id.clone());
        rows.push(Row::Message(Box::new(row)));
    }
    rows
}

pub fn thread_rows(records: &[MessageRecord], root_id: &str, context: &RowContext) -> Vec<Row> {
    let members: Vec<MessageRecord> = records
        .iter()
        .filter(|record| {
            record.message_id == root_id || record.reply_to_id.as_deref() == Some(root_id)
        })
        .cloned()
        .collect();
    flat_rows(&members, context, false)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Splice {
    pub range: Range<usize>,
    pub count: usize,
}

pub fn diff_keys(old: &[String], new: &[String]) -> Option<Splice> {
    if old == new {
        return None;
    }
    let prefix = old
        .iter()
        .zip(new)
        .take_while(|(left, right)| left == right)
        .count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(left, right)| left == right)
        .count();
    Some(Splice {
        range: prefix..old.len() - suffix,
        count: new.len() - prefix - suffix,
    })
}

pub fn changed_indices(old: &[Row], new: &[Row]) -> Vec<usize> {
    let prefix = old
        .iter()
        .zip(new)
        .take_while(|(left, right)| left.key() == right.key())
        .count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(left, right)| left.key() == right.key())
        .count();
    let mut changed: Vec<usize> = (0..prefix)
        .filter(|&index| old[index] != new[index])
        .collect();
    changed.extend((0..suffix).filter_map(|offset| {
        let old_index = old.len() - 1 - offset;
        let new_index = new.len() - 1 - offset;
        (old[old_index] != new[new_index]).then_some(new_index)
    }));
    changed
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn record(id: &str, reply_to: Option<&str>, hour: u32, day: u32) -> MessageRecord {
        MessageRecord {
            conversation_id: "c".into(),
            message_id: id.into(),
            reply_to_id: reply_to.map(str::to_owned),
            sender_id: Some("u".into()),
            sender_name: Some("Author".into()),
            created_at: Utc.with_ymd_and_hms(2026, 10, day, hour, 0, 0).unwrap(),
            edited_at: None,
            deleted: false,
            body_html: format!("<p>text {id}</p>"),
            attachments_json: "[]".into(),
            reactions_json: "[]".into(),
            mentions_json: "[]".into(),
        }
    }

    #[test]
    fn reply_excerpt_uses_the_first_line_or_the_attachment() {
        let mut text = record("a", None, 8, 6);
        text.body_html = "<p>first line</p><p>second</p>".into();
        assert_eq!(reply_excerpt(&text), "first line");
        let mut quoted = record("q", None, 8, 6);
        quoted.body_html = "<blockquote>Jonas<br>old text</blockquote><p>my answer</p>".into();
        assert_eq!(reply_excerpt(&quoted), "my answer");
        let mut image = record("b", None, 8, 6);
        image.body_html = "<img src=\"u\">".into();
        assert_eq!(reply_excerpt(&image), "Image");
        let mut long = record("c", None, 8, 6);
        long.body_html = format!("<p>{}</p>", "x".repeat(200));
        assert!(reply_excerpt(&long).ends_with("..."));
        assert_eq!(
            reply_excerpt(&long).chars().count(),
            REPLY_EXCERPT_CHARS + 3
        );
    }

    fn context() -> RowContext {
        RowContext {
            offset: FixedOffset::east_opt(0).unwrap(),
            today: NaiveDate::from_ymd_opt(2026, 10, 7).unwrap(),
            my_user_id: None,
        }
    }

    fn keys(rows: &[Row]) -> Vec<String> {
        rows.iter().map(|row| row.key().to_owned()).collect()
    }

    #[test]
    fn day_header_only_on_first_message_of_a_day() {
        let records = vec![
            record("a", None, 8, 6),
            record("b", None, 9, 6),
            record("c", None, 9, 7),
        ];
        let rows = flat_rows(&records, &context(), false);
        let headers: Vec<Option<String>> = rows
            .iter()
            .map(|row| match row {
                Row::Message(message) => message.day_header.clone(),
                Row::LoadOlder | Row::Start(_) | Row::Skeleton(_) => None,
            })
            .collect();
        assert_eq!(
            headers,
            vec![Some("Yesterday".into()), None, Some("Today".into())]
        );
    }

    #[test]
    fn load_older_row_only_when_more_exists() {
        let records = vec![record("a", None, 8, 6)];
        assert_eq!(
            keys(&flat_rows(&records, &context(), true))[0],
            LOAD_OLDER_KEY
        );
        assert_eq!(keys(&flat_rows(&records, &context(), false)), vec!["a"]);
    }

    #[test]
    fn threads_group_replies_by_root() {
        let records = vec![
            record("r1", None, 8, 6),
            record("r2", None, 9, 6),
            record("x1", Some("r1"), 10, 6),
            record("x2", Some("r1"), 11, 6),
            record("orphan", Some("missing"), 12, 6),
        ];
        let threads = group_threads(&records);
        assert_eq!(threads.len(), 2);
        assert_eq!(threads[0].reply_ids, vec!["x1", "x2"]);
        assert!(threads[1].reply_ids.is_empty());
    }

    #[test]
    fn thread_list_orders_by_last_activity_with_reply_count() {
        let records = vec![
            record("r1", None, 8, 6),
            record("r2", None, 9, 6),
            record("x1", Some("r1"), 10, 6),
        ];
        let rows = thread_list_rows(&records, &context(), false);
        assert_eq!(keys(&rows), vec!["r2", "r1"]);
        let Row::Message(last) = &rows[1] else {
            panic!("message expected")
        };
        assert_eq!(last.reply_count, Some(1));
        assert_eq!(last.open_thread.as_deref(), Some("r1"));
    }

    #[test]
    fn thread_view_has_root_then_replies() {
        let records = vec![
            record("r1", None, 8, 6),
            record("r2", None, 9, 6),
            record("x1", Some("r1"), 10, 6),
        ];
        assert_eq!(
            keys(&thread_rows(&records, "r1", &context())),
            vec!["r1", "x1"]
        );
    }

    #[test]
    fn reactions_are_counted_in_first_seen_order() {
        let reaction = |kind: &str| ReactionInfo {
            reaction_type: kind.into(),
            user_id: None,
            user_name: None,
        };
        let chips = reaction_chips(
            &[reaction("heart"), reaction("like"), reaction("heart")],
            None,
        );
        assert_eq!(chips.len(), 2);
        assert_eq!(chips[0].count, 2);
        assert_eq!(chips[1].label, reaction_label("like"));
    }

    fn record_by(id: &str, sender: &str, hour: u32, minute: u32, day: u32) -> MessageRecord {
        let mut record = record(id, None, hour, day);
        record.sender_id = Some(sender.into());
        record.created_at = Utc
            .with_ymd_and_hms(2026, 10, day, hour, minute, 0)
            .unwrap();
        record
    }

    fn series_of(rows: &[Row]) -> Vec<(bool, bool)> {
        rows.iter()
            .filter_map(|row| match row {
                Row::Message(message) => Some((message.series.has_prev, message.series.has_next)),
                Row::LoadOlder | Row::Start(_) | Row::Skeleton(_) => None,
            })
            .collect()
    }

    #[test]
    fn series_joins_same_sender_within_five_minutes() {
        let records = vec![
            record_by("a", "ada", 9, 0, 7),
            record_by("b", "ada", 9, 4, 7),
            record_by("c", "ada", 9, 20, 7),
            record_by("d", "bob", 9, 21, 7),
            record_by("e", "ada", 9, 22, 7),
        ];
        let rows = flat_rows(&records, &context(), false);
        assert_eq!(
            series_of(&rows),
            vec![
                (false, true),
                (true, false),
                (false, false),
                (false, false),
                (false, false)
            ]
        );
    }

    #[test]
    fn series_window_ends_exactly_at_the_gap_constant() {
        let gap = SERIES_GAP_MINUTES as u32;
        let records = vec![
            record_by("a", "ada", 9, 0, 7),
            record_by("b", "ada", 9, gap - 1, 7),
            record_by("c", "ada", 9, 2 * gap - 1 + gap, 7),
        ];
        let rows = flat_rows(&records, &context(), false);
        assert_eq!(
            series_of(&rows),
            vec![(false, true), (true, false), (false, false)]
        );
    }

    #[test]
    fn series_breaks_at_a_new_day() {
        let records = vec![
            record_by("a", "ada", 23, 58, 6),
            record_by("b", "ada", 0, 1, 7),
        ];
        let rows = flat_rows(&records, &context(), false);
        assert_eq!(series_of(&rows), vec![(false, false), (false, false)]);
    }

    #[test]
    fn thread_list_rows_never_join() {
        let records = vec![
            record_by("a", "ada", 9, 0, 7),
            record_by("b", "ada", 9, 1, 7),
        ];
        let rows = thread_list_rows(&records, &context(), false);
        assert_eq!(series_of(&rows), vec![(false, false), (false, false)]);
    }

    fn corners(own: bool, has_prev: bool, has_next: bool) -> [f32; 4] {
        let corners = bubble_corners(own, Series { has_prev, has_next });
        [
            corners.top_left,
            corners.top_right,
            corners.bottom_right,
            corners.bottom_left,
        ]
    }

    #[test]
    fn corners_match_the_mockup_for_other_senders() {
        assert_eq!(corners(false, false, false), [4., 14., 14., 14.]);
        assert_eq!(corners(false, false, true), [4., 14., 14., 4.]);
        assert_eq!(corners(false, true, true), [4., 14., 14., 4.]);
        assert_eq!(corners(false, true, false), [4., 14., 14., 14.]);
    }

    #[test]
    fn corners_match_the_mockup_for_own_messages() {
        assert_eq!(corners(true, false, false), [14., 4., 4., 14.]);
        assert_eq!(corners(true, false, true), [14., 14., 4., 14.]);
        assert_eq!(corners(true, true, true), [14., 4., 4., 14.]);
        assert_eq!(corners(true, true, false), [14., 4., 14., 14.]);
    }

    #[test]
    fn own_reaction_is_marked() {
        let reaction = |user: &str| ReactionInfo {
            reaction_type: "like".into(),
            user_id: Some(user.into()),
            user_name: None,
        };
        let chips = reaction_chips(&[reaction("x"), reaction("me")], Some("me"));
        assert!(chips[0].mine);
        assert!(!reaction_chips(&[reaction("x")], Some("me"))[0].mine);
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn diff_detects_append_prepend_and_replace() {
        let base = strings(&["a", "b"]);
        assert_eq!(diff_keys(&base, &base), None);
        assert_eq!(
            diff_keys(&base, &strings(&["a", "b", "c"])),
            Some(Splice {
                range: 2..2,
                count: 1
            })
        );
        assert_eq!(
            diff_keys(&base, &strings(&["x", "a", "b"])),
            Some(Splice {
                range: 0..0,
                count: 1
            })
        );
        assert_eq!(
            diff_keys(
                &strings(&["load-older", "a"]),
                &strings(&["load-older", "x", "y", "a"])
            ),
            Some(Splice {
                range: 1..1,
                count: 2
            })
        );
        assert_eq!(
            diff_keys(&strings(&["load-older", "a"]), &strings(&["a"])),
            Some(Splice {
                range: 0..1,
                count: 0
            })
        );
        assert_eq!(
            diff_keys(&base, &strings(&["x"])),
            Some(Splice {
                range: 0..2,
                count: 1
            })
        );
    }

    #[test]
    fn changed_indices_find_edited_rows() {
        let old = flat_rows(
            &[record("a", None, 8, 6), record("b", None, 9, 6)],
            &context(),
            false,
        );
        let mut edited = record("b", None, 9, 6);
        edited.body_html = "<p>changed</p>".into();
        let new = flat_rows(&[record("a", None, 8, 6), edited], &context(), false);
        assert_eq!(changed_indices(&old, &new), vec![1]);
    }
}
