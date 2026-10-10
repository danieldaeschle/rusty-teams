use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::sync::Arc;

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, Utc};
use gpui_kit::Image;
use store::MessageRecord;
use teams_core::{
    AdaptiveCard, Draft, FileCard, ImageRef, LinkPreview, ReactionInfo, Span, adaptive_cards,
    card_texts, files, images, link_preview, linked_message_spans, meeting_link_in_html,
    message_spans, reactions, translated_spans,
};

use crate::card_state::CardOverride;
use crate::format;
use crate::reaction_model::{Reactor, UNKNOWN_REACTOR};
use crate::render::blocks::{Inline, strip_image_placeholders};
use crate::render::{Block, layout_blocks};
use crate::translation::{TranslationContext, TranslationLine};

const POST_VISIBLE_REPLIES: usize = 3;

pub const LOAD_OLDER_KEY: &str = "load-older";
const UNKNOWN_AUTHOR: &str = "Unknown";
const DELETED_TEXT: &str = "Message deleted";
const SERIES_GAP_MINUTES: i64 = 5;
const CORNER_RADIUS: f32 = 14.;
const CORNER_SERIES: f32 = 4.;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReactionChip {
    pub reaction_type: String,
    pub label: String,
    pub count: usize,
    pub mine: bool,
    pub reactors: Vec<Reactor>,
}

impl ReactionChip {
    pub fn glyph(&self) -> String {
        reaction_glyph(&self.reaction_type)
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduledState {
    Waiting,
    DeliveryFailed,
    ChangeFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
    Delivered,
    Sending,
    Failed(String),
    Scheduled(ScheduledState),
}

/// An image of a message that is still being sent, shown from its local bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalImage {
    pub image: Arc<Image>,
    pub size: Option<(u32, u32)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MessageRow {
    pub key: String,
    pub conversation_id: String,
    pub author: String,
    pub sender_id: Option<String>,
    pub application_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub series: Series,
    pub time: String,
    pub day_header: Option<String>,
    pub blocks: Vec<Block>,
    pub edited: bool,
    pub deleted: bool,
    pub reactions: Vec<ReactionChip>,
    pub images: Vec<ImageRef>,
    pub local_images: Vec<LocalImage>,
    pub files: Vec<FileCard>,
    pub link_preview: Option<LinkPreview>,
    pub meeting_link: Option<String>,
    pub adaptive_cards: Vec<AdaptiveCard>,
    pub subject: Option<String>,
    pub new_marker: bool,
    pub reply_root: Option<String>,
    pub delivery: Delivery,
    pub own: bool,
    pub receipt: Receipt,
    pub forwarded: bool,
    pub translation: Option<TranslationLine>,
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
    Post(Box<PostRow>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Skeleton {
    pub key: &'static str,
    pub own: bool,
    pub width_ratio: f32,
    pub lines: u8,
}

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

fn sender_key(row: &MessageRow) -> &str {
    row.sender_id.as_deref().unwrap_or(&row.author)
}

fn continues_series(previous: &MessageRow, next: &MessageRow) -> bool {
    next.day_header.is_none()
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
            Row::Post(post) => &post.root.key,
        }
    }

    pub fn messages(&self) -> Box<dyn Iterator<Item = &MessageRow> + '_> {
        match self {
            Row::Message(message) => Box::new(std::iter::once(message.as_ref())),
            Row::Post(post) => Box::new(post.messages()),
            Row::LoadOlder | Row::Start(_) | Row::Skeleton(_) => Box::new(std::iter::empty()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PostRow {
    pub root: MessageRow,
    pub replies: Vec<MessageRow>,
    pub hidden_reply_count: usize,
    pub hidden_repliers: Vec<String>,
}

impl PostRow {
    pub fn local(root: MessageRow) -> Self {
        PostRow {
            root,
            replies: Vec::new(),
            hidden_reply_count: 0,
            hidden_repliers: Vec::new(),
        }
    }

    pub fn messages(&self) -> impl Iterator<Item = &MessageRow> {
        std::iter::once(&self.root).chain(&self.replies)
    }

    pub fn hidden_replies_label(&self) -> Option<String> {
        (self.hidden_reply_count > 0)
            .then(|| replies_label(self.hidden_reply_count, &self.hidden_repliers))
    }
}

fn first_name(name: &str) -> &str {
    name.split_whitespace().next().unwrap_or(name)
}

pub fn replies_label(count: usize, repliers: &[String]) -> String {
    let noun = if count == 1 { "reply" } else { "replies" };
    let mut names: Vec<&str> = Vec::new();
    for name in repliers.iter().map(|name| first_name(name)) {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    let people = match names.as_slice() {
        [] => return format!("{count} {noun}"),
        [only] => (*only).to_owned(),
        [first, second] => format!("{first} and {second}"),
        [first, second, third] => format!("{first}, {second} and {third}"),
        [first, second, rest @ ..] => format!("{first}, {second} and {} others", rest.len()),
    };
    format!("{count} {noun} from {people}")
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
        "laugh" => "\u{1F606}".to_owned(),
        "surprised" => "\u{1F62E}".to_owned(),
        "sad" => "\u{1F641}".to_owned(),
        "angry" => "\u{1F620}".to_owned(),
        other => other.to_owned(),
    }
}

pub fn reaction_glyph(reaction_type: &str) -> String {
    reaction_label(reaction_type)
        .trim_end_matches('\u{FE0F}')
        .to_owned()
}

/// Graph rejects the legacy names ("Unicode 'like' in the payload is not supported").
pub fn api_reaction(glyph: &str) -> String {
    match glyph {
        "\u{2764}" => "\u{2764}\u{FE0F}".to_owned(),
        other => other.to_owned(),
    }
}

/// The reaction type Teams uses for a glyph: the legacy name for the six classic ones.
pub fn reaction_type_for(glyph: &str) -> String {
    let bare = glyph.trim_end_matches('\u{FE0F}');
    ["like", "heart", "laugh", "surprised", "sad", "angry"]
        .into_iter()
        .find(|name| reaction_label(name) == bare)
        .map_or_else(|| bare.to_owned(), str::to_owned)
}

pub type PendingReactions = HashMap<String, Vec<(String, bool)>>;

fn is_own_reaction(reaction: &ReactionInfo, wanted: &str, my_user_id: Option<&str>) -> bool {
    my_user_id.is_some()
        && reaction.user_id.as_deref() == my_user_id
        && reaction_glyph(&reaction.reaction_type) == wanted
}

pub fn has_own_reaction(reactions: &[ReactionInfo], glyph: &str, my_user_id: Option<&str>) -> bool {
    let wanted = reaction_glyph(glyph);
    reactions
        .iter()
        .any(|reaction| is_own_reaction(reaction, &wanted, my_user_id))
}

pub fn set_own_reaction(
    reactions: &mut Vec<ReactionInfo>,
    glyph: &str,
    my_user_id: Option<&str>,
    present: bool,
    now: DateTime<Utc>,
) {
    let wanted = reaction_glyph(glyph);
    if !present {
        reactions.retain(|reaction| !is_own_reaction(reaction, &wanted, my_user_id));
    } else if !has_own_reaction(reactions, glyph, my_user_id) {
        reactions.push(ReactionInfo {
            reaction_type: reaction_type_for(glyph),
            user_id: my_user_id.map(str::to_owned),
            user_name: Some("You".to_owned()),
            created_at: Some(now),
        });
    }
}

pub fn apply_pending_reactions(
    reactions: &mut Vec<ReactionInfo>,
    pending: Option<&Vec<(String, bool)>>,
    my_user_id: Option<&str>,
    now: DateTime<Utc>,
) {
    for (glyph, present) in pending.into_iter().flatten() {
        set_own_reaction(reactions, glyph, my_user_id, *present, now);
    }
}

pub fn reaction_chips(reactions: &[ReactionInfo], context: &RowContext) -> Vec<ReactionChip> {
    let my_user_id = context.my_user_id.as_deref();
    let mut chips: Vec<ReactionChip> = Vec::new();
    for reaction in reactions {
        let glyph = reaction_glyph(&reaction.reaction_type);
        let position = match chips.iter().position(|chip| chip.glyph() == glyph) {
            Some(position) => position,
            None => {
                chips.push(ReactionChip {
                    reaction_type: reaction.reaction_type.clone(),
                    label: reaction_label(&reaction.reaction_type),
                    count: 0,
                    mine: false,
                    reactors: Vec::new(),
                });
                chips.len() - 1
            }
        };
        let chip = &mut chips[position];
        chip.count += 1;
        chip.mine |= my_user_id.is_some() && reaction.user_id.as_deref() == my_user_id;
        chip.reactors.push(Reactor {
            user_id: reaction.user_id.clone(),
            name: reactor_name(reaction, context),
            created_at: reaction.created_at,
        });
    }
    chips
}

fn reactor_name(reaction: &ReactionInfo, context: &RowContext) -> String {
    reaction
        .user_name
        .clone()
        .or_else(|| {
            reaction
                .user_id
                .as_ref()
                .and_then(|user_id| context.names.get(user_id).cloned())
        })
        .unwrap_or_else(|| UNKNOWN_REACTOR.to_owned())
}

const REPLY_EXCERPT_CHARS: usize = 90;
const FORWARD_ITEMTYPE: &str = "schema.skype.com/Forward";

pub fn message_text(record: &MessageRecord) -> String {
    let own_spans: Vec<Span> = message_spans(record)
        .into_iter()
        .filter(|span| !matches!(span, Span::Quote(_)))
        .collect();
    strip_image_placeholders(layout_blocks(&own_spans))
        .into_iter()
        .filter_map(|block| block.markdown())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The message as the composer edits it, formatting kept.
pub fn message_draft(record: &MessageRecord) -> Draft {
    let own_spans: Vec<Span> = message_spans(record)
        .into_iter()
        .filter(|span| !matches!(span, Span::Quote(_)))
        .collect();
    Draft::from_spans(&own_spans).trimmed()
}

pub fn reply_excerpt(record: &MessageRecord) -> String {
    let own_spans: Vec<Span> = message_spans(record)
        .into_iter()
        .filter(|span| !matches!(span, Span::Quote(_)))
        .collect();
    let text = strip_image_placeholders(layout_blocks(&own_spans))
        .into_iter()
        .find_map(|block| block.first_line())
        .unwrap_or_default();
    let first_line = text.lines().map(str::trim).find(|line| !line.is_empty());
    let excerpt = match first_line {
        Some(line) => line.to_owned(),
        None if !images(record).is_empty() => "Image".to_owned(),
        None if let Some(line) = card_texts(record)
            .iter()
            .flat_map(|text| text.lines())
            .next() =>
        {
            line.to_owned()
        }
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
    pub names: HashMap<String, String>,
    pub pending_reactions: PendingReactions,
    pub card_overrides: HashMap<String, CardOverride>,
    pub translation: TranslationContext,
}

fn visible_reactions(record: &MessageRecord, context: &RowContext) -> Vec<ReactionInfo> {
    let mut visible = reactions(record);
    apply_pending_reactions(
        &mut visible,
        context.pending_reactions.get(&record.message_id),
        context.my_user_id.as_deref(),
        Utc::now(),
    );
    visible
}

pub fn message_row(record: &MessageRecord, context: &RowContext) -> MessageRow {
    let local = record.created_at.with_timezone(&context.offset);
    let images = images(record);
    let own = context.my_user_id.is_some() && record.sender_id == context.my_user_id;
    let translation = context.translation.resolve(record, own);
    let blocks = if record.deleted {
        vec![Block::Paragraph(Inline::plain(DELETED_TEXT))]
    } else if let Some(html) = &translation.translated_html {
        layout_blocks(&translated_spans(record, html))
    } else {
        layout_blocks(&linked_message_spans(record))
    };
    let blocks = if images.is_empty() {
        blocks
    } else {
        strip_image_placeholders(blocks)
    };
    MessageRow {
        key: record.message_id.clone(),
        conversation_id: record.conversation_id.clone(),
        sender_id: record.sender_id.clone(),
        application_id: record.sender_application_id.clone(),
        created_at: record.created_at,
        series: Series::default(),
        author: record
            .sender_name
            .clone()
            .unwrap_or_else(|| UNKNOWN_AUTHOR.to_owned()),
        time: clock_label(record.created_at, context.offset),
        day_header: Some(format::day_label(local.date_naive(), context.today)),
        blocks,
        edited: record.edited_at.is_some() && !record.deleted,
        deleted: record.deleted,
        reactions: reaction_chips(&visible_reactions(record, context), context),
        images,
        local_images: Vec::new(),
        files: files(record),
        link_preview: link_preview(record),
        meeting_link: (!record.deleted)
            .then(|| meeting_link_in_html(&record.body_html))
            .flatten(),
        adaptive_cards: match context.card_overrides.get(&record.message_id) {
            Some(replaced) if replaced.basis == record.attachments_json => replaced.cards.clone(),
            _ => adaptive_cards(record),
        },
        subject: record.subject.clone(),
        new_marker: false,
        reply_root: record.reply_to_id.clone(),
        delivery: Delivery::Delivered,
        receipt: Receipt::Hidden,
        own,
        forwarded: !record.deleted && is_forwarded(&record.body_html),
        translation: translation.line,
    }
}

pub fn is_forwarded(body_html: &str) -> bool {
    body_html.contains(FORWARD_ITEMTYPE)
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

fn post_message_row(record: &MessageRecord, context: &RowContext) -> MessageRow {
    let mut row = message_row(record, context);
    row.day_header = None;
    row.time = format::post_time_label(record.created_at, context.today, context.offset);
    row
}

fn distinct_names<'a>(records: impl Iterator<Item = &'a MessageRecord>) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for record in records {
        let name = record
            .sender_name
            .clone()
            .unwrap_or_else(|| UNKNOWN_AUTHOR.to_owned());
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

fn post_row(
    root: &MessageRecord,
    mut replies: Vec<&MessageRecord>,
    context: &RowContext,
) -> PostRow {
    replies.sort_by_key(|reply| reply.created_at);
    let hidden = replies.len().saturating_sub(POST_VISIBLE_REPLIES);
    PostRow {
        root: post_message_row(root, context),
        hidden_reply_count: hidden,
        hidden_repliers: distinct_names(replies[..hidden].iter().copied()),
        replies: replies[hidden..]
            .iter()
            .map(|reply| post_message_row(reply, context))
            .collect(),
    }
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
    threads.sort_by_key(|thread| std::cmp::Reverse(last_activity(thread, &by_id)));
    let mut rows: Vec<Row> = threads
        .iter()
        .filter_map(|thread| {
            let root = by_id.get(thread.root_id.as_str())?;
            let replies = thread
                .reply_ids
                .iter()
                .filter_map(|id| by_id.get(id.as_str()).copied())
                .collect();
            Some(Row::Post(Box::new(post_row(root, replies, context))))
        })
        .collect();
    if has_older {
        rows.push(Row::LoadOlder);
    }
    rows
}

pub fn place_local_rows(rows: &mut Vec<Row>, local: Vec<MessageRow>) {
    for message in local {
        let Some(root_id) = message.reply_root.clone() else {
            rows.insert(0, Row::Post(Box::new(PostRow::local(message))));
            continue;
        };
        let position = rows
            .iter()
            .position(|row| matches!(row, Row::Post(post) if post.root.key == root_id));
        if let Some(position) = position {
            let mut row = rows.remove(position);
            if let Row::Post(post) = &mut row {
                post.replies.push(message);
            }
            rows.insert(0, row);
        }
    }
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
            sender_application_id: None,
            links_json: "[]".to_owned(),
            created_at: Utc.with_ymd_and_hms(2026, 10, day, hour, 0, 0).unwrap(),
            edited_at: None,
            deleted: false,
            body_html: format!("<p>text {id}</p>"),
            attachments_json: "[]".into(),
            reactions_json: "[]".into(),
            mentions_json: "[]".into(),
            subject: None,
        }
    }

    fn own_reaction(reaction_type: &str) -> ReactionInfo {
        ReactionInfo {
            reaction_type: reaction_type.into(),
            user_id: Some("me".into()),
            user_name: None,
            created_at: None,
        }
    }

    #[test]
    fn setting_a_reaction_is_idempotent_and_only_touches_mine() {
        let now = Utc.with_ymd_and_hms(2026, 10, 1, 8, 0, 0).unwrap();
        let other = ReactionInfo {
            user_id: Some("them".into()),
            ..own_reaction("like")
        };
        let mut reactions = vec![other.clone()];
        set_own_reaction(&mut reactions, "\u{1F44D}", Some("me"), true, now);
        set_own_reaction(&mut reactions, "\u{1F44D}", Some("me"), true, now);
        assert_eq!(reactions.len(), 2);
        assert_eq!(reactions[1].reaction_type, "like");
        assert_eq!(reactions[1].user_id.as_deref(), Some("me"));
        set_own_reaction(&mut reactions, "\u{1F44D}", Some("me"), false, now);
        set_own_reaction(&mut reactions, "\u{1F44D}", Some("me"), false, now);
        assert_eq!(reactions, vec![other]);
    }

    #[test]
    fn setting_matches_legacy_names_and_variation_selectors() {
        let now = Utc.with_ymd_and_hms(2026, 10, 1, 8, 0, 0).unwrap();
        let mut reactions = vec![own_reaction("heart")];
        assert!(has_own_reaction(&reactions, "\u{2764}\u{FE0F}", Some("me")));
        set_own_reaction(&mut reactions, "\u{2764}\u{FE0F}", Some("me"), false, now);
        assert!(reactions.is_empty());
        set_own_reaction(&mut reactions, "\u{1F389}", Some("me"), true, now);
        assert_eq!(reactions[0].reaction_type, "\u{1F389}");
    }

    #[test]
    fn setting_without_a_known_user_never_matches() {
        let now = Utc.with_ymd_and_hms(2026, 10, 1, 8, 0, 0).unwrap();
        let mut reactions = Vec::new();
        set_own_reaction(&mut reactions, "\u{1F44D}", None, true, now);
        set_own_reaction(&mut reactions, "\u{1F44D}", None, true, now);
        assert_eq!(reactions.len(), 2);
    }

    #[test]
    fn pending_reactions_overlay_the_stored_ones() {
        let mut stored = record("a", None, 8, 6);
        stored.reactions_json = serde_json::to_string(&[own_reaction("like")]).unwrap();
        let mut context = context();
        context.my_user_id = Some("me".into());
        context.pending_reactions.insert(
            "a".into(),
            vec![("\u{1F44D}".into(), false), ("\u{1F389}".into(), true)],
        );
        let chips = message_row(&stored, &context).reactions;
        assert_eq!(chips.len(), 1);
        assert_eq!(chips[0].reaction_type, "\u{1F389}");
        assert!(chips[0].mine);
    }

    #[test]
    fn reaction_type_maps_classic_glyphs_to_names() {
        assert_eq!(reaction_type_for("\u{1F44D}"), "like");
        assert_eq!(reaction_type_for("\u{2764}\u{FE0F}"), "heart");
        assert_eq!(reaction_type_for("\u{1F389}"), "\u{1F389}");
        assert_eq!(reaction_glyph("like"), reaction_glyph("\u{1F44D}"));
        assert_eq!(reaction_glyph("heart"), reaction_glyph("\u{2764}\u{FE0F}"));
        assert_eq!(api_reaction(&reaction_glyph("heart")), "\u{2764}\u{FE0F}");
        assert_eq!(api_reaction(&reaction_glyph("like")), "\u{1F44D}");
    }

    #[test]
    fn forwarded_messages_are_detected_by_the_forward_blockquote() {
        assert!(is_forwarded(
            r#"<p>fyi</p><blockquote itemscope="" itemtype="http://schema.skype.com/Forward" itemid="1">text</blockquote>"#
        ));
        assert!(!is_forwarded(
            r#"<blockquote itemtype="http://schema.skype.com/Reply">text</blockquote>"#
        ));
        assert!(!is_forwarded("<p>plain</p>"));
        let mut forwarded = record("f", None, 8, 6);
        forwarded.body_html =
            r#"<blockquote itemtype="http://schema.skype.com/Forward">text</blockquote>"#.into();
        assert!(message_row(&forwarded, &context()).forwarded);
        forwarded.deleted = true;
        assert!(!message_row(&forwarded, &context()).forwarded);
    }

    #[test]
    fn editing_loads_the_message_formatted_without_its_reply_quote() {
        let mut edited = record("a", None, 8, 6);
        edited.body_html =
            "<blockquote itemtype=\"http://schema.skype.com/Reply\">old</blockquote>\
                            <p><b>Plan</b> for <a href=\"https://x.test\">today</a></p>\
                            <ol><li>one</li><li>two</li></ol>"
                .into();
        let draft = message_draft(&edited);
        assert_eq!(draft.text(), "Plan for today\n1. one\n2. two");
        assert_eq!(
            draft.to_html(),
            "<b>Plan</b> for <a href=\"https://x.test\">today</a><ol><li>one</li><li>two</li></ol>"
        );
    }

    #[test]
    fn reply_excerpt_uses_the_first_line_or_the_attachment() {
        let mut text = record("a", None, 8, 6);
        text.body_html = "<p>first line</p><p>second</p>".into();
        assert_eq!(reply_excerpt(&text), "first line");
        let mut quoted = record("q", None, 8, 6);
        quoted.body_html = r#"<blockquote itemtype="http://schema.skype.com/Reply">Jonas<br>old text</blockquote><p>my answer</p>"#.into();
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
            names: HashMap::new(),
            pending_reactions: HashMap::new(),
            card_overrides: HashMap::new(),
            translation: TranslationContext::default(),
        }
    }

    fn keys(rows: &[Row]) -> Vec<String> {
        rows.iter().map(|row| row.key().to_owned()).collect()
    }

    fn translation_context(shown: bool) -> RowContext {
        use crate::translation::CachedTranslation;
        let mut context = context();
        context.translation.enabled = true;
        context.translation.target = "en".into();
        context.translation.stamps.insert(
            "a".into(),
            "languages=fr:100;length:80;&detector=Bling".into(),
        );
        context.my_user_id = Some("me".into());
        if shown {
            context.translation.cached.insert(
                ("a".into(), 0),
                CachedTranslation {
                    html: "<p><strong>translated</strong> body</p>".into(),
                    source: Some("fr".into()),
                },
            );
            context.translation.shown.insert("a".into());
        }
        context
    }

    #[test]
    fn a_foreign_message_carries_the_offer_and_keeps_its_text() {
        let row = message_row(&record("a", None, 8, 6), &translation_context(false));
        assert!(matches!(
            row.translation,
            Some(TranslationLine::Offer { ref language_code, .. }) if language_code == "fr"
        ));
        assert_eq!(row.blocks, layout_blocks(&linked_message_spans(&record("a", None, 8, 6))));
    }

    #[test]
    fn a_shown_translation_replaces_the_body_and_the_original_comes_back() {
        let original = message_row(&record("a", None, 8, 6), &translation_context(false)).blocks;
        let translated = message_row(&record("a", None, 8, 6), &translation_context(true));
        assert_ne!(translated.blocks, original);
        assert_eq!(
            translated.translation,
            Some(TranslationLine::Translated {
                language: Some("French".into()),
                showing_original: false
            })
        );
        let mut hidden = translation_context(true);
        hidden.translation.shown.clear();
        let back = message_row(&record("a", None, 8, 6), &hidden);
        assert_eq!(back.blocks, original);
        assert_eq!(
            back.translation,
            Some(TranslationLine::Translated {
                language: Some("French".into()),
                showing_original: true
            })
        );
    }

    #[test]
    fn own_messages_get_no_offer() {
        let mut own = record("a", None, 8, 6);
        own.sender_id = Some("me".into());
        assert_eq!(message_row(&own, &translation_context(false)).translation, None);
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
                Row::LoadOlder | Row::Start(_) | Row::Skeleton(_) | Row::Post(_) => None,
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

    fn post_of(row: &Row) -> &PostRow {
        let Row::Post(post) = row else {
            panic!("post expected")
        };
        post
    }

    fn reply_by(id: &str, root: &str, sender: &str, hour: u32) -> MessageRecord {
        let mut reply = record(id, Some(root), hour, 6);
        reply.sender_name = Some(sender.into());
        reply
    }

    #[test]
    fn feed_orders_posts_by_last_activity_newest_first() {
        let records = vec![
            record("r1", None, 8, 6),
            record("r2", None, 9, 6),
            record("r3", None, 10, 6),
            record("x1", Some("r1"), 11, 6),
        ];
        let rows = thread_list_rows(&records, &context(), false);
        assert_eq!(keys(&rows), vec!["r1", "r3", "r2"]);
    }

    #[test]
    fn feed_puts_load_older_at_the_bottom() {
        let records = vec![record("r1", None, 8, 6), record("r2", None, 9, 6)];
        let rows = thread_list_rows(&records, &context(), true);
        assert_eq!(keys(&rows), vec!["r2", "r1", LOAD_OLDER_KEY]);
    }

    #[test]
    fn post_shows_the_last_three_replies_oldest_first() {
        let mut records = vec![record("r1", None, 1, 6)];
        for hour in 2..=6 {
            records.push(reply_by(&format!("x{hour}"), "r1", "Ada Lovelace", hour));
        }
        records.reverse();
        let rows = thread_list_rows(&records, &context(), false);
        let post = post_of(&rows[0]);
        let shown: Vec<&str> = post
            .replies
            .iter()
            .map(|reply| reply.key.as_str())
            .collect();
        assert_eq!(shown, vec!["x4", "x5", "x6"]);
        assert_eq!(post.hidden_reply_count, 2);
        assert_eq!(post.hidden_repliers, vec!["Ada Lovelace"]);
    }

    #[test]
    fn post_without_hidden_replies_has_no_link() {
        let records = vec![
            record("r1", None, 1, 6),
            reply_by("x1", "r1", "Ada", 2),
            reply_by("x2", "r1", "Ben", 3),
            reply_by("x3", "r1", "Cy", 4),
        ];
        let rows = thread_list_rows(&records, &context(), false);
        assert_eq!(post_of(&rows[0]).hidden_replies_label(), None);
        assert_eq!(post_of(&rows[0]).replies.len(), 3);
    }

    fn label_for(repliers: &[&str]) -> String {
        let mut records = vec![record("r1", None, 1, 6)];
        for (index, name) in repliers.iter().enumerate() {
            records.push(reply_by(&format!("h{index}"), "r1", name, 2 + index as u32));
        }
        for index in 0..POST_VISIBLE_REPLIES {
            records.push(reply_by(
                &format!("v{index}"),
                "r1",
                "Shown",
                20 + index as u32,
            ));
        }
        let rows = thread_list_rows(&records, &context(), false);
        post_of(&rows[0]).hidden_replies_label().unwrap()
    }

    #[test]
    fn replies_label_names_one_to_three_people() {
        assert_eq!(label_for(&["Anna Meier"]), "1 reply from Anna");
        assert_eq!(
            label_for(&["Anna Meier", "Anna Meier", "Ben Roth"]),
            "3 replies from Anna and Ben"
        );
        assert_eq!(
            label_for(&["Anna Meier", "Ben Roth", "Clara Voss", "Ben Roth"]),
            "4 replies from Anna, Ben and Clara"
        );
    }

    #[test]
    fn replies_label_counts_the_others_beyond_three_people() {
        assert_eq!(
            label_for(&["Anna Meier", "Ben Roth", "Clara Voss", "Dan Fox", "Eve Ray"]),
            "5 replies from Anna, Ben and 3 others"
        );
        assert_eq!(
            label_for(&["Anna Meier", "Ben Roth", "Clara Voss", "Dan Fox"]),
            "4 replies from Anna, Ben and 2 others"
        );
    }

    #[test]
    fn local_reply_lands_in_its_post_and_local_post_on_top() {
        let records = vec![
            record("r1", None, 8, 6),
            record("r2", None, 9, 6),
            reply_by("x1", "r1", "Ada", 10),
        ];
        let mut rows = thread_list_rows(&records, &context(), true);
        let mut pending_reply = message_row(&record("pending-1", Some("r2"), 12, 6), &context());
        pending_reply.reply_root = Some("r2".into());
        let pending_post = message_row(&record("pending-2", None, 12, 6), &context());
        place_local_rows(&mut rows, vec![pending_reply, pending_post]);
        assert_eq!(keys(&rows), vec!["pending-2", "r2", "r1", LOAD_OLDER_KEY]);
        let replies: Vec<&str> = post_of(&rows[1])
            .replies
            .iter()
            .map(|reply| reply.key.as_str())
            .collect();
        assert_eq!(replies, vec!["pending-1"]);
    }

    #[test]
    fn local_reply_to_an_unloaded_post_is_not_shown() {
        let mut rows = thread_list_rows(&[record("r1", None, 8, 6)], &context(), false);
        let mut orphan = message_row(&record("pending-1", Some("gone"), 12, 6), &context());
        orphan.reply_root = Some("gone".into());
        place_local_rows(&mut rows, vec![orphan]);
        assert_eq!(keys(&rows), vec!["r1"]);
    }

    #[test]
    fn subject_reaches_the_post_root() {
        let mut with_subject = record("r1", None, 8, 6);
        with_subject.subject = Some("Release plan".into());
        let rows = thread_list_rows(&[with_subject, record("r2", None, 7, 6)], &context(), false);
        assert_eq!(
            post_of(&rows[0]).root.subject.as_deref(),
            Some("Release plan")
        );
        assert_eq!(post_of(&rows[1]).root.subject, None);
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
            created_at: None,
        };
        let chips = reaction_chips(
            &[reaction("heart"), reaction("like"), reaction("heart")],
            &context(),
        );
        assert_eq!(chips.len(), 2);
        assert_eq!(chips[0].count, 2);
        assert_eq!(chips[1].label, reaction_label("like"));
    }

    #[test]
    fn legacy_laugh_and_sad_use_the_teams_glyphs() {
        assert_eq!(reaction_label("laugh"), "\u{1F606}");
        assert_eq!(reaction_label("sad"), "\u{1F641}");
        assert_eq!(reaction_type_for("\u{1F606}"), "laugh");
        assert_eq!(reaction_type_for("\u{1F641}"), "sad");
    }

    #[test]
    fn legacy_and_emoji_reactions_share_a_chip() {
        let reaction = |kind: &str| ReactionInfo {
            reaction_type: kind.into(),
            user_id: None,
            user_name: Some("Lea".into()),
            created_at: None,
        };
        let chips = reaction_chips(&[reaction("like"), reaction("\u{1F44D}")], &context());
        assert_eq!(chips.len(), 1);
        assert_eq!(chips[0].count, 2);
    }

    #[test]
    fn reactor_name_falls_back_to_directory_then_unknown() {
        let reaction = |user: &str| ReactionInfo {
            reaction_type: "like".into(),
            user_id: Some(user.into()),
            user_name: None,
            created_at: None,
        };
        let mut context = context();
        context.names.insert("known".into(), "Priya Nair".into());
        let chips = reaction_chips(&[reaction("known"), reaction("other")], &context);
        assert_eq!(chips[0].reactors[0].name, "Priya Nair");
        assert_eq!(chips[0].reactors[1].name, "Unknown");
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
                Row::LoadOlder | Row::Start(_) | Row::Skeleton(_) | Row::Post(_) => None,
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
            created_at: None,
        };
        let mut context = context();
        context.my_user_id = Some("me".into());
        let chips = reaction_chips(&[reaction("x"), reaction("me")], &context);
        assert!(chips[0].mine);
        assert!(!reaction_chips(&[reaction("x")], &context)[0].mine);
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
