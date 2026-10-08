use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::markdown::escape_html;
use crate::spans::Span;

pub const MAX_DEPTH: u8 = 2;
pub const OBJECT_MARK: char = '\u{FFFC}';
const BULLETS: [&str; 3] = ["•", "◦", "▪"];
const PASTED_BULLETS: [&str; 6] = ["- ", "* ", "+ ", "• ", "◦ ", "▪ "];
const LINK_SCHEMES: [&str; 3] = ["http://", "https://", "mailto:"];
const AUTOLINK_PREFIXES: [&str; 4] = ["http://", "https://", "www.", "mailto:"];
const AUTOLINK_TRAILING: &str = ".,;:!?'\"";
const FENCE: &str = "```";
const MARKDOWN_INDENT: usize = 2;

static TYPED_LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^\]\n]+)\]\(([^)\s]+)\)$").unwrap());
static LINK_AT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[([^\]\n]+)\]\(([^)\s]+)\)").unwrap());

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum MarkKind {
    Link(String),
    Bold,
    Italic,
    Underline,
    Strike,
    Code,
}

impl MarkKind {
    pub fn same_format(&self, other: &MarkKind) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }

    fn continues_at_end(&self) -> bool {
        matches!(
            self,
            MarkKind::Bold | MarkKind::Italic | MarkKind::Underline | MarkKind::Strike
        )
    }

    fn open_tag(&self) -> String {
        match self {
            MarkKind::Link(url) => format!("<a href=\"{}\">", escape_html(url)),
            MarkKind::Bold => "<b>".to_owned(),
            MarkKind::Italic => "<i>".to_owned(),
            MarkKind::Underline => "<u>".to_owned(),
            MarkKind::Strike => "<s>".to_owned(),
            MarkKind::Code => "<code>".to_owned(),
        }
    }

    fn close_tag(&self) -> &'static str {
        match self {
            MarkKind::Link(_) => "</a>",
            MarkKind::Bold => "</b>",
            MarkKind::Italic => "</i>",
            MarkKind::Underline => "</u>",
            MarkKind::Strike => "</s>",
            MarkKind::Code => "</code>",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    pub range: Range<usize>,
    pub kind: MarkKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineKind {
    Text,
    Bullet(u8),
    Numbered(u8),
    Quote,
    Code(Option<String>),
}

impl LineKind {
    pub fn is_list(&self) -> bool {
        matches!(self, LineKind::Bullet(_) | LineKind::Numbered(_))
    }

    pub fn depth(&self) -> u8 {
        match self {
            LineKind::Bullet(depth) | LineKind::Numbered(depth) => *depth,
            _ => 0,
        }
    }

    pub fn same_format(&self, other: &LineKind) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }

    fn with_depth(&self, depth: u8) -> LineKind {
        match self {
            LineKind::Bullet(_) => LineKind::Bullet(depth),
            LineKind::Numbered(_) => LineKind::Numbered(depth),
            other => other.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatState {
    Off,
    Mixed,
    On,
}

/// Formats switched on or off for text typed at `at` before any is typed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TypingStyle {
    pub at: usize,
    pub add: Vec<MarkKind>,
    pub remove: Vec<MarkKind>,
}

impl TypingStyle {
    pub fn apply(&self, mut kinds: Vec<MarkKind>) -> Vec<MarkKind> {
        kinds.retain(|kind| !self.remove.iter().any(|removed| removed.same_format(kind)));
        for added in &self.add {
            if !kinds.iter().any(|kind| kind.same_format(added)) {
                kinds.push(added.clone());
            }
        }
        kinds
    }

    pub fn toggle(&mut self, kind: MarkKind, active: &[MarkKind]) {
        let is_active = self
            .apply(active.to_vec())
            .iter()
            .any(|existing| existing.same_format(&kind));
        self.add.retain(|added| !added.same_format(&kind));
        self.remove.retain(|removed| !removed.same_format(&kind));
        let inherited = active.iter().any(|existing| existing.same_format(&kind));
        match (is_active, inherited) {
            (true, true) => self.remove.push(kind),
            (false, false) => self.add.push(kind),
            _ => {}
        }
    }

    pub fn is_empty(&self) -> bool {
        self.add.is_empty() && self.remove.is_empty()
    }
}

/// One line without its marker: what the clipboard and `from_lines` carry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftLine {
    pub kind: LineKind,
    pub number: Option<u32>,
    pub content: String,
    pub marks: Vec<Mark>,
}

impl DraftLine {
    fn text(content: &str) -> DraftLine {
        DraftLine {
            kind: LineKind::Text,
            number: None,
            content: content.to_owned(),
            marks: Vec::new(),
        }
    }

    fn trim_start(&mut self) {
        let cut = self.content.len() - self.content.trim_start().len();
        self.content.drain(..cut);
        for mark in &mut self.marks {
            mark.range = mark.range.start.saturating_sub(cut)..mark.range.end.saturating_sub(cut);
        }
        self.marks.retain(|mark| !mark.range.is_empty());
    }

    fn trim_end(&mut self) {
        let length = self.content.trim_end().len();
        self.content.truncate(length);
        for mark in &mut self.marks {
            mark.range = mark.range.start.min(length)..mark.range.end.min(length);
        }
        self.marks.retain(|mark| !mark.range.is_empty());
    }
}

pub type Edit = (Range<usize>, String);

/// Edits since the last `take_edits`, as disjoint ranges of the text at that point.
#[derive(Debug, Clone, Default)]
struct Journal(Vec<Edit>);

impl PartialEq for Journal {
    fn eq(&self, _: &Journal) -> bool {
        true
    }
}

impl Eq for Journal {}

impl Journal {
    fn record(&mut self, range: Range<usize>, inserted: usize, text: &str) {
        if range.is_empty() && inserted == 0 {
            return;
        }
        let mut delta = 0isize;
        let mut delta_before = 0isize;
        let mut position = self.0.len();
        let mut touching: Vec<(usize, usize, usize)> = Vec::new();
        for (index, (original, replacement)) in self.0.iter().enumerate() {
            let current_start = (original.start as isize + delta) as usize;
            let current_end = current_start + replacement.len();
            if current_end < range.start {
                delta += replacement.len() as isize - original.len() as isize;
                delta_before = delta;
                continue;
            }
            if current_start > range.end {
                position = position.min(index);
                break;
            }
            position = position.min(index);
            touching.push((index, current_start, current_end));
            delta += replacement.len() as isize - original.len() as isize;
        }
        let original_start = match touching.first() {
            Some(&(index, current_start, _)) if current_start <= range.start => {
                self.0[index].0.start
            }
            _ => (range.start as isize - delta_before) as usize,
        };
        let original_end = match touching.last() {
            Some(&(index, _, current_end)) if current_end >= range.end => self.0[index].0.end,
            _ => (range.end as isize - delta) as usize,
        };
        let merged_start = touching
            .first()
            .map_or(range.start, |&(_, current_start, _)| {
                current_start.min(range.start)
            });
        let merged_end = touching
            .last()
            .map_or(range.end, |&(_, _, current_end)| current_end.max(range.end));
        let merged_end_after = merged_end + inserted - range.len();
        let replacement = text[merged_start..merged_end_after].to_owned();
        if let (Some(&(first, ..)), Some(&(last, ..))) = (touching.first(), touching.last()) {
            self.0.drain(first..=last);
        }
        self.0
            .insert(position, (original_start..original_end, replacement));
    }
}

/// Where `offset` of the text before `edits` ends up; an insertion at it pushes it along.
pub fn map_offset(edits: &[Edit], offset: usize) -> usize {
    let mut delta = 0isize;
    for (original, replacement) in edits {
        if offset >= original.end {
            delta += replacement.len() as isize - original.len() as isize;
        } else if offset > original.start {
            return (original.start as isize + delta) as usize + replacement.len();
        } else {
            break;
        }
    }
    (offset as isize + delta) as usize
}

/// The edits that turn the text after `edits` back into `before`.
pub fn reverse_edits(edits: &[Edit], before: &str) -> Vec<Edit> {
    let mut delta = 0isize;
    edits
        .iter()
        .map(|(original, replacement)| {
            let start = (original.start as isize + delta) as usize;
            delta += replacement.len() as isize - original.len() as isize;
            (
                start..start + replacement.len(),
                before[original.clone()].to_owned(),
            )
        })
        .collect()
}

/// Composer text plus formatting. List and quote lines carry their marker in `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    text: String,
    marks: Vec<Mark>,
    lines: Vec<LineKind>,
    journal: Journal,
}

impl Default for Draft {
    fn default() -> Self {
        Draft::plain("")
    }
}

impl Draft {
    pub fn plain(text: &str) -> Draft {
        Draft {
            text: text.to_owned(),
            marks: Vec::new(),
            lines: vec![LineKind::Text; text.matches('\n').count() + 1],
            journal: Journal::default(),
        }
    }

    pub fn from_lines(lines: &[DraftLine]) -> Draft {
        let mut draft = Draft::plain("");
        draft.insert_lines(0..0, lines);
        draft.journal = Journal::default();
        draft
    }

    pub fn from_markdown(text: &str) -> Draft {
        Draft::from_lines(&markdown_lines(text))
    }

    pub fn from_spans(spans: &[Span]) -> Draft {
        let mut builder = SpanLines::default();
        builder.walk(spans, &LineKind::Text, &[]);
        Draft::from_lines(&builder.finish())
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn marks(&self) -> &[Mark] {
        &self.marks
    }

    pub fn lines(&self) -> &[LineKind] {
        &self.lines
    }

    pub fn take_edits(&mut self) -> Vec<Edit> {
        std::mem::take(&mut self.journal.0)
    }

    pub fn is_blank(&self) -> bool {
        self.plain_text().trim().is_empty()
    }

    /// The text without list and quote markers and object marks.
    pub fn plain_text(&self) -> String {
        self.slice(0..self.text.len())
            .into_iter()
            .map(|line| line.content)
            .collect::<Vec<_>>()
            .join("\n")
            .replace(&format!(" {OBJECT_MARK} "), " ")
            .replace(OBJECT_MARK, "")
    }

    pub fn line_ranges(&self) -> Vec<Range<usize>> {
        let mut ranges = Vec::with_capacity(self.lines.len());
        let mut start = 0;
        for (index, _) in self.text.match_indices('\n') {
            ranges.push(start..index);
            start = index + 1;
        }
        ranges.push(start..self.text.len());
        ranges
    }

    pub fn line_at(&self, offset: usize) -> usize {
        let offset = offset.min(self.text.len());
        self.text.as_bytes()[..offset]
            .iter()
            .filter(|byte| **byte == b'\n')
            .count()
    }

    pub fn marker_range(&self, index: usize) -> Range<usize> {
        let line = self.line_ranges()[index].clone();
        let length = marker_len(&self.lines[index], &self.text[line.clone()]);
        line.start..line.start + length
    }

    pub fn content_range(&self, index: usize) -> Range<usize> {
        let line = self.line_ranges()[index].clone();
        self.marker_range(index).end..line.end
    }

    /// Inside a code block line, or strictly inside inline code.
    pub fn in_code(&self, offset: usize) -> bool {
        matches!(self.lines[self.line_at(offset)], LineKind::Code(_))
            || self.marks.iter().any(|mark| {
                mark.kind == MarkKind::Code && mark.range.start < offset && offset < mark.range.end
            })
    }

    /// Formats that text typed at `offset` takes on.
    pub fn style_at(&self, offset: usize) -> Vec<MarkKind> {
        self.style_for_insertion(offset, offset)
    }

    pub fn state(&self, range: Range<usize>, kind: &MarkKind) -> FormatState {
        let parts = self.content_parts(range);
        let total: usize = parts.iter().map(|part| part.len()).sum();
        if total == 0 {
            return FormatState::Off;
        }
        let covered: usize = parts
            .iter()
            .map(|part| {
                let mut ranges: Vec<Range<usize>> = self
                    .marks
                    .iter()
                    .filter(|mark| mark.kind.same_format(kind))
                    .map(|mark| mark.range.start.max(part.start)..mark.range.end.min(part.end))
                    .filter(|range| !range.is_empty())
                    .collect();
                ranges.sort_by_key(|range| range.start);
                let mut end = part.start;
                let mut sum = 0;
                for range in ranges {
                    let start = range.start.max(end);
                    if range.end > start {
                        sum += range.end - start;
                        end = range.end;
                    }
                }
                sum
            })
            .sum();
        match covered {
            0 => FormatState::Off,
            covered if covered >= total => FormatState::On,
            _ => FormatState::Mixed,
        }
    }

    /// A line the selection only touches at its start does not count.
    fn selected_lines(&self, range: &Range<usize>) -> std::ops::RangeInclusive<usize> {
        let first = self.line_at(range.start);
        let mut last = self.line_at(range.end);
        if last > first && self.line_ranges()[last].start == range.end {
            last -= 1;
        }
        first..=last
    }

    pub fn line_state(&self, range: Range<usize>, kind: &LineKind) -> FormatState {
        let lines = self.selected_lines(&range);
        let count = lines.clone().count();
        let matching = lines
            .filter(|index| self.lines[*index].same_format(kind))
            .count();
        match matching {
            0 => FormatState::Off,
            matching if matching == count => FormatState::On,
            _ => FormatState::Mixed,
        }
    }

    pub fn link_in(&self, range: Range<usize>) -> Option<String> {
        self.marks.iter().find_map(|mark| match &mark.kind {
            MarkKind::Link(url)
                if mark.range.start < range.end.max(range.start + 1)
                    && range.start < mark.range.end =>
            {
                Some(url.clone())
            }
            _ => None,
        })
    }

    /// Mixed or off: the whole range gets `kind`. On: it loses it.
    pub fn toggle(&mut self, range: Range<usize>, kind: MarkKind) {
        self.journal = Journal::default();
        if self.state(range.clone(), &kind) == FormatState::On {
            self.remove_marks(range, &kind);
        } else {
            for part in self.content_parts(range) {
                self.add_mark(part, kind.clone());
            }
        }
    }

    /// An empty `url` removes the link; one `link_url` refuses changes nothing.
    pub fn set_link(&mut self, range: Range<usize>, url: &str) {
        self.journal = Journal::default();
        let url = url.trim();
        if !url.is_empty() && link_url(url).is_none() {
            return;
        }
        let link = MarkKind::Link(url.to_owned());
        self.remove_marks(range.clone(), &link);
        if !url.is_empty() {
            for part in self.content_parts(range) {
                self.add_mark(part, link.clone());
            }
        }
    }

    /// Returns the offset after the inserted link text; plain text when `link_url` refuses `url`.
    pub fn insert_link(&mut self, offset: usize, text: &str, url: &str) -> usize {
        self.journal = Journal::default();
        self.splice(offset..offset, text, &[]);
        self.add_mark(offset..offset + text.len(), MarkKind::Link(url.to_owned()));
        offset + text.len()
    }

    /// Replaces `range`, the new text taking on the format of the text it replaces.
    pub fn replace(&mut self, range: Range<usize>, text: &str) {
        self.journal = Journal::default();
        let kinds = self.style_for_insertion(range.start, range.end);
        self.splice(range, text, &kinds);
    }

    /// Takes the composer text after an edit the user made. Returns the cursor, moved when a
    /// marker had to be repaired or the typed text moved out of one.
    pub fn apply_edit(
        &mut self,
        new_text: &str,
        cursor: usize,
        typing: Option<&TypingStyle>,
    ) -> usize {
        self.journal = Journal::default();
        if new_text == self.text {
            return cursor;
        }
        let (start, old_end, new_end) = edit_span(&self.text, new_text, cursor);
        let replacement = new_text[start..new_end].to_owned();
        let mut kinds = self.style_for_insertion(start, old_end);
        if let Some(typing) = typing.filter(|typing| typing.at == start) {
            kinds = typing.apply(kinds);
        }
        let marker = self.marker_range(self.line_at(start));
        let auto_urls = self.auto_link_urls(&(start..old_end));
        self.splice(start..old_end, &replacement, &kinds);
        self.retarget_links(&auto_urls, &(start..start + replacement.len()));
        self.journal = Journal::default();
        let mut cursor = cursor.min(self.text.len());
        let typed_into_marker = old_end == start
            && !replacement.contains('\n')
            && !marker.is_empty()
            && (marker.start..marker.end).contains(&start);
        if typed_into_marker {
            self.splice(start..start + replacement.len(), "", &[]);
            self.splice(marker.end..marker.end, &replacement, &kinds);
            cursor = marker.end + replacement.len();
        }
        self.repair(cursor)
    }

    /// Markdown typed just before `cursor` turns into formatting. Returns the new cursor and
    /// the style for what is typed next, so text after `**bold**` is not bold.
    pub fn convert_typed(&mut self, cursor: usize) -> Option<(usize, TypingStyle)> {
        self.journal = Journal::default();
        let index = self.line_at(cursor);
        if matches!(self.lines[index], LineKind::Code(_)) {
            return None;
        }
        let content = self.content_range(index);
        if cursor < content.start || cursor > content.end {
            return None;
        }
        let before = &self.text[content.start..cursor];
        if self.lines[index] == LineKind::Text
            && let Some((kind, number)) = line_shortcut(before)
        {
            self.splice(content.start..cursor, &marker_text(&kind, number), &[]);
            self.lines[index] = kind;
            let after_marker = self.content_range(index).start;
            let cursor = self.repair(after_marker);
            return Some((
                cursor,
                TypingStyle {
                    at: cursor,
                    ..TypingStyle::default()
                },
            ));
        }
        let (open, close, kind) = typed_inline(before)?;
        let open = content.start + open.start..content.start + open.end;
        let close = content.start + close.start..content.start + close.end;
        let inside_code = self.marks.iter().any(|mark| {
            mark.kind == MarkKind::Code
                && (overlaps(&mark.range, &open) || overlaps(&mark.range, &close))
        });
        if inside_code {
            return None;
        }
        self.splice(close.clone(), "", &[]);
        self.splice(open.clone(), "", &[]);
        let inner = open.start..close.start - open.len();
        self.add_mark(inner, kind.clone());
        let cursor = cursor - open.len() - close.len();
        Some((
            cursor,
            TypingStyle {
                at: cursor,
                add: Vec::new(),
                remove: vec![kind],
            },
        ))
    }

    /// A URL typed just before the space or newline at `cursor - 1` becomes a link.
    pub fn autolink_typed(&mut self, cursor: usize) -> bool {
        self.journal = Journal::default();
        let Some(end) = cursor.checked_sub(1) else {
            return false;
        };
        if !matches!(self.text.get(end..cursor), Some(" " | "\n")) {
            return false;
        }
        self.link_word_ending_at(end)
    }

    /// Links the URL in the word that ends at `end`. Leaves the journal alone.
    pub fn link_word_ending_at(&mut self, end: usize) -> bool {
        let index = self.line_at(end);
        if matches!(self.lines[index], LineKind::Code(_)) {
            return false;
        }
        let content_start = self.content_range(index).start;
        let Some(before) = self.text.get(content_start..end) else {
            return false;
        };
        let word_start = before.rfind(char::is_whitespace).map_or(0, |at| {
            at + before[at..].chars().next().map_or(1, char::len_utf8)
        });
        let word = &before[word_start..];
        let Some(found) = word.char_indices().find_map(|(offset, _)| {
            let previous = word[..offset].chars().next_back();
            let length = bare_url_at(&word[offset..], previous)?;
            Some(offset..offset + length)
        }) else {
            return false;
        };
        let range =
            content_start + word_start + found.start..content_start + word_start + found.end;
        let taken = self.marks.iter().any(|mark| {
            matches!(mark.kind, MarkKind::Code | MarkKind::Link(_)) && overlaps(&mark.range, &range)
        });
        let Some(url) = link_url(&self.text[range.clone()]).filter(|_| !taken) else {
            return false;
        };
        self.add_mark(range, MarkKind::Link(url));
        true
    }

    /// Enter (or Shift+Enter) inside a list, quote or code block, or after a ``` fence.
    /// `None` leaves the key to the caller: send, or a plain newline.
    pub fn break_line(&mut self, cursor: usize, shift: bool) -> Option<usize> {
        self.journal = Journal::default();
        let index = self.line_at(cursor);
        let kind = self.lines[index].clone();
        let content = self.content_range(index);
        let cursor = cursor.max(content.start);
        match kind {
            LineKind::Text => {
                let language = fence_language(&self.text[content.clone()])?;
                self.splice(content.clone(), "", &[]);
                Some(self.set_line_kind(index, LineKind::Code(language), None, content.start))
            }
            LineKind::Quote if !shift => None,
            LineKind::Quote => {
                self.splice(cursor..cursor, "\n", &[]);
                self.lines[index] = LineKind::Quote;
                self.lines[index + 1] = LineKind::Quote;
                Some(cursor + 1)
            }
            LineKind::Code(_) => {
                let last_of_block = self.lines.get(index + 1) != Some(&kind);
                let block_continues_above = index > 0 && self.lines[index - 1] == kind;
                if content.is_empty() && last_of_block && block_continues_above && !shift {
                    return Some(self.set_line_kind(index, LineKind::Text, None, cursor));
                }
                self.splice(cursor..cursor, "\n", &[]);
                Some(cursor + 1)
            }
            list => {
                if self.text[content.clone()].trim().is_empty() {
                    let lifted = match list.depth() {
                        0 => LineKind::Text,
                        depth => list.with_depth(depth - 1),
                    };
                    let cursor = self.set_line_kind(index, lifted, None, cursor);
                    return Some(self.repair(cursor));
                }
                self.splice(cursor..cursor, "\n", &[]);
                self.lines[index + 1] = list;
                let marker = self.expected_marker(index + 1, None);
                self.splice(cursor + 1..cursor + 1, &marker, &[]);
                Some(self.repair(cursor + 1 + marker.len()))
            }
        }
    }

    /// Forward Delete next to a marker: at a line end it joins the next line without its
    /// marker; inside a marker it deletes the first character of the content instead.
    pub fn delete_forward(&mut self, cursor: usize) -> Option<usize> {
        self.journal = Journal::default();
        let index = self.line_at(cursor);
        let line = self.line_ranges()[index].clone();
        let content = self.content_range(index);
        if cursor >= line.start && cursor < content.start {
            let next = self.text[content.start..]
                .chars()
                .next()
                .filter(|character| *character != '\n');
            match next {
                Some(character) => {
                    self.splice(content.start..content.start + character.len_utf8(), "", &[]);
                }
                None if index + 1 < self.lines.len() => {
                    let next_content = self.content_range(index + 1);
                    self.splice(content.end..next_content.start, "", &[]);
                }
                None => return Some(content.start),
            }
            return Some(self.repair(content.start));
        }
        if cursor != line.end || index + 1 >= self.lines.len() {
            return None;
        }
        let next_marker = self.marker_range(index + 1);
        if next_marker.is_empty() {
            return None;
        }
        self.splice(cursor..next_marker.end, "", &[]);
        Some(self.repair(cursor))
    }

    /// Backspace at the start of a formatted line drops (or lifts) its format.
    pub fn backspace_at_start(&mut self, cursor: usize) -> Option<usize> {
        self.journal = Journal::default();
        let index = self.line_at(cursor);
        let kind = self.lines[index].clone();
        let line = self.line_ranges()[index].clone();
        let content = self.content_range(index);
        match &kind {
            LineKind::Text => None,
            LineKind::Code(_) => {
                let first_of_block = index == 0 || self.lines[index - 1] != kind;
                (cursor == line.start && first_of_block).then(|| {
                    self.lines[index] = LineKind::Text;
                    cursor
                })
            }
            LineKind::Quote
                if cursor == line.start && (index == 0 || self.lines[index - 1] != kind) =>
            {
                self.lines[index] = LineKind::Text;
                Some(cursor)
            }
            _ if cursor == line.start && index > 0 => {
                self.splice(line.start - 1..content.start, "", &[]);
                Some(self.repair(line.start - 1))
            }
            _ if cursor > line.start && cursor <= content.start => {
                let lifted = match kind.depth() {
                    depth if depth > 0 => kind.with_depth(depth - 1),
                    _ => LineKind::Text,
                };
                let cursor = self.set_line_kind(index, lifted, None, cursor);
                Some(self.repair(cursor))
            }
            _ => None,
        }
    }

    /// Tab / Shift+Tab on list lines. `false` when a selected line is not a list item.
    pub fn indent(&mut self, range: Range<usize>, outdent: bool) -> bool {
        self.journal = Journal::default();
        let lines = self.selected_lines(&range);
        if !lines.clone().all(|index| self.lines[index].is_list()) {
            return false;
        }
        for index in lines {
            let kind = self.lines[index].clone();
            let depth = kind.depth();
            let target = match (outdent, depth) {
                (true, 0) => LineKind::Text,
                (true, depth) => kind.with_depth(depth - 1),
                (false, depth) => kind.with_depth((depth + 1).min(MAX_DEPTH)),
            };
            self.set_line_kind(index, target, None, 0);
        }
        self.repair(0);
        true
    }

    /// All selected lines `kind` already: back to text. Otherwise all become `kind`.
    pub fn toggle_lines(&mut self, range: Range<usize>, kind: LineKind) {
        self.journal = Journal::default();
        let lines = self.selected_lines(&range);
        let all = lines
            .clone()
            .all(|index| self.lines[index].same_format(&kind));
        for index in lines {
            let current = self.lines[index].clone();
            let target = if all {
                LineKind::Text
            } else if current.same_format(&kind) {
                current.clone()
            } else if current.is_list() && kind.is_list() {
                kind.with_depth(current.depth())
            } else {
                kind.clone()
            };
            if target != current {
                self.set_line_kind(index, target, None, 0);
            }
        }
        self.repair(0);
    }

    pub fn slice(&self, range: Range<usize>) -> Vec<DraftLine> {
        let mut lines = Vec::new();
        for (index, line) in self.line_ranges().into_iter().enumerate() {
            if line.end < range.start || line.start > range.end {
                continue;
            }
            let content = self.content_range(index);
            let whole_start = range.start <= content.start;
            let start = range.start.max(content.start);
            let end = range.end.min(content.end).max(start);
            let kind = if whole_start {
                self.lines[index].clone()
            } else {
                LineKind::Text
            };
            let number = match kind {
                LineKind::Numbered(_) => {
                    numbered_marker(&self.text[line.clone()]).map(|(_, number)| number)
                }
                _ => None,
            };
            let marks = self
                .marks
                .iter()
                .filter_map(|mark| {
                    let clipped = mark.range.start.max(start)..mark.range.end.min(end);
                    (!clipped.is_empty()).then(|| Mark {
                        range: clipped.start - start..clipped.end - start,
                        kind: mark.kind.clone(),
                    })
                })
                .collect();
            lines.push(DraftLine {
                kind,
                number,
                content: self.text[start..end].to_owned(),
                marks,
            });
        }
        lines
    }

    pub fn slice_without_objects(&self, range: Range<usize>) -> Vec<DraftLine> {
        let mut copy = Draft::from_lines(&self.slice(range));
        let marks: Vec<usize> = copy
            .text
            .match_indices(OBJECT_MARK)
            .map(|(index, _)| index)
            .collect();
        for index in marks.into_iter().rev() {
            copy.replace(index..index + OBJECT_MARK.len_utf8(), "");
        }
        copy.slice(0..usize::MAX)
    }

    /// Puts `lines` in place of `range`. Returns the offset after the inserted content.
    pub fn insert_lines(&mut self, range: Range<usize>, lines: &[DraftLine]) -> usize {
        self.journal = Journal::default();
        let index = self.line_at(range.start);
        let content = self.content_range(index);
        let start = range.start.max(content.start);
        let range = start..range.end.max(start);
        let tail_end = self.line_ranges()[self.line_at(range.end)].end;
        let line_was_empty = self.lines[index] == LineKind::Text
            && range.start == content.start
            && range.end >= tail_end;
        let joined = lines
            .iter()
            .map(|line| line.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        self.splice(range.clone(), &joined, &[]);
        let mut offset = range.start;
        for line in lines {
            for mark in &line.marks {
                self.add_mark(
                    offset + mark.range.start..offset + mark.range.end,
                    mark.kind.clone(),
                );
            }
            offset += line.content.len() + 1;
        }
        let length_before_markers = self.text.len();
        for (position, line) in lines.iter().enumerate() {
            let first_and_kept = position == 0 && !line_was_empty;
            if first_and_kept || line.kind == self.lines[index + position] {
                continue;
            }
            self.set_line_kind(index + position, line.kind.clone(), line.number, 0);
        }
        let end =
            (range.start + joined.len() + self.text.len()).saturating_sub(length_before_markers);
        self.repair(end)
    }

    /// Leading and trailing blank lines and spaces dropped.
    pub fn trimmed(&self) -> Draft {
        let mut lines = self.slice(0..self.text.len());
        let blank =
            |line: &DraftLine| line.kind == LineKind::Text && line.content.trim().is_empty();
        while lines.len() > 1 && lines.first().is_some_and(blank) {
            lines.remove(0);
        }
        while lines.len() > 1 && lines.last().is_some_and(blank) {
            lines.pop();
        }
        if let Some(first) = lines.first_mut().filter(|line| line.kind == LineKind::Text) {
            first.trim_start();
        }
        if let Some(last) = lines
            .last_mut()
            .filter(|line| !matches!(line.kind, LineKind::Code(_)))
        {
            last.trim_end();
        }
        Draft::from_lines(&lines)
    }

    /// Teams chat HTML.
    pub fn to_html(&self) -> String {
        let lines = self.slice(0..self.text.len());
        let mut html = String::new();
        let mut index = 0;
        while index < lines.len() {
            let kind = &lines[index].kind;
            let run_end = index
                + lines[index..]
                    .iter()
                    .take_while(|line| match kind {
                        LineKind::Code(_) => line.kind == *kind,
                        _ if kind.is_list() => line.kind.is_list(),
                        _ => line.kind.same_format(kind),
                    })
                    .count();
            let run = &lines[index..run_end];
            match kind {
                LineKind::Text => html.push_str(&joined_inline(run)),
                LineKind::Quote => {
                    html.push_str("<blockquote>");
                    html.push_str(&joined_inline(run));
                    html.push_str("</blockquote>");
                }
                LineKind::Code(language) => {
                    match language {
                        Some(language) => html.push_str(&format!(
                            "<pre class=\"language-{}\">",
                            escape_html(language)
                        )),
                        None => html.push_str("<pre>"),
                    }
                    let code = run
                        .iter()
                        .map(|line| escape_html(&line.content))
                        .collect::<Vec<_>>()
                        .join("\n");
                    html.push_str(&code);
                    html.push_str("</pre>");
                }
                _ => html.push_str(&list_html(run)),
            }
            index = run_end;
        }
        html
    }

    fn style_for_insertion(&self, start: usize, old_end: usize) -> Vec<MarkKind> {
        let mut kinds: Vec<MarkKind> = self
            .marks
            .iter()
            .filter(|mark| {
                let range = &mark.range;
                if old_end > start {
                    range.start <= start && start < range.end
                } else {
                    (range.start < start && start < range.end)
                        || (range.start < start
                            && range.end == start
                            && mark.kind.continues_at_end())
                }
            })
            .map(|mark| mark.kind.clone())
            .collect();
        kinds.sort();
        kinds.dedup();
        kinds
    }

    fn auto_link_urls(&self, range: &Range<usize>) -> Vec<String> {
        self.marks
            .iter()
            .filter_map(|mark| match &mark.kind {
                MarkKind::Link(url)
                    if mark.range.start <= range.end
                        && range.start <= mark.range.end
                        && autolink_prefix(&self.text[mark.range.clone()]).is_some()
                        && link_url(&self.text[mark.range.clone()]).as_ref() == Some(url) =>
                {
                    Some(url.clone())
                }
                _ => None,
            })
            .collect()
    }

    fn retarget_links(&mut self, auto_urls: &[String], edited: &Range<usize>) {
        if auto_urls.is_empty() {
            return;
        }
        let mut kept = Vec::with_capacity(self.marks.len());
        for mark in std::mem::take(&mut self.marks) {
            let touched = mark.range.start <= edited.end && edited.start <= mark.range.end;
            match &mark.kind {
                MarkKind::Link(url) if touched && auto_urls.contains(url) => {
                    if let Some(new_url) = link_url(&self.text[mark.range.clone()]) {
                        kept.push(Mark {
                            range: mark.range,
                            kind: MarkKind::Link(new_url),
                        });
                    }
                }
                _ => kept.push(mark),
            }
        }
        self.marks = kept;
        self.normalize_marks();
    }

    fn content_parts(&self, range: Range<usize>) -> Vec<Range<usize>> {
        (self.line_at(range.start)..=self.line_at(range.end))
            .map(|index| {
                let content = self.content_range(index);
                content.start.max(range.start)..content.end.min(range.end)
            })
            .filter(|part| !part.is_empty())
            .collect()
    }

    /// Links with a scheme other than http, https or mailto stay plain text.
    fn add_mark(&mut self, range: Range<usize>, kind: MarkKind) {
        let kind = match kind {
            MarkKind::Link(url) => match link_url(&url) {
                Some(url) => MarkKind::Link(url),
                None => return,
            },
            other => other,
        };
        if !range.is_empty() {
            self.marks.push(Mark { range, kind });
            self.normalize_marks();
        }
    }

    fn remove_marks(&mut self, range: Range<usize>, kind: &MarkKind) {
        let mut kept = Vec::with_capacity(self.marks.len());
        for mark in std::mem::take(&mut self.marks) {
            if !mark.kind.same_format(kind) || !overlaps(&mark.range, &range) {
                kept.push(mark);
                continue;
            }
            if mark.range.start < range.start {
                kept.push(Mark {
                    range: mark.range.start..range.start,
                    kind: mark.kind.clone(),
                });
            }
            if mark.range.end > range.end {
                kept.push(Mark {
                    range: range.end..mark.range.end,
                    kind: mark.kind,
                });
            }
        }
        self.marks = kept;
        self.normalize_marks();
    }

    fn normalize_marks(&mut self) {
        let length = self.text.len();
        let mut marks: Vec<Mark> = std::mem::take(&mut self.marks)
            .into_iter()
            .map(|mark| Mark {
                range: mark.range.start.min(length)..mark.range.end.min(length),
                kind: mark.kind,
            })
            .filter(|mark| !mark.range.is_empty())
            .collect();
        marks.sort_by(|left, right| {
            left.kind
                .cmp(&right.kind)
                .then(left.range.start.cmp(&right.range.start))
        });
        let mut merged: Vec<Mark> = Vec::with_capacity(marks.len());
        for mark in marks {
            match merged.last_mut() {
                Some(last) if last.kind == mark.kind && mark.range.start <= last.range.end => {
                    last.range.end = last.range.end.max(mark.range.end);
                }
                _ => merged.push(mark),
            }
        }
        self.marks = merged;
    }

    fn splice(&mut self, range: Range<usize>, replacement: &str, kinds: &[MarkKind]) {
        let (start, old_end) = (range.start, range.end);
        let first_line = self.line_at(start);
        let last_line = self.line_at(old_end);
        let line_start = self.line_ranges()[first_line].start;
        let breaks = replacement.matches('\n').count();
        let kind = self.lines[first_line].clone();
        let mut new_kinds = Vec::with_capacity(breaks + 1);
        let pushes_line_down = breaks > 0
            && start == line_start
            && old_end == start
            && !matches!(kind, LineKind::Code(_))
            && self.text[start..]
                .chars()
                .next()
                .is_some_and(|next| next != '\n');
        if pushes_line_down {
            new_kinds.extend(std::iter::repeat_n(LineKind::Text, breaks));
            new_kinds.push(kind);
        } else {
            let continued = match &kind {
                LineKind::Code(language) => LineKind::Code(language.clone()),
                _ => LineKind::Text,
            };
            new_kinds.push(kind);
            new_kinds.extend(std::iter::repeat_n(continued, breaks));
        }
        self.lines.splice(first_line..=last_line, new_kinds);
        self.text.replace_range(range.clone(), replacement);

        let inserted = replacement.len();
        let delta = inserted as isize - (old_end - start) as isize;
        let shift = |position: usize| (position as isize + delta) as usize;
        let map_start = |position: usize| {
            if position < start {
                position
            } else if position >= old_end {
                shift(position)
            } else {
                start + inserted
            }
        };
        let map_end = |position: usize| {
            if position <= start {
                position
            } else if position >= old_end {
                shift(position)
            } else {
                start
            }
        };
        for mark in &mut self.marks {
            mark.range = map_start(mark.range.start)..map_end(mark.range.end);
        }
        if inserted > 0 {
            let inserted_range = start..start + inserted;
            let mut kept = Vec::with_capacity(self.marks.len() + kinds.len());
            for mark in std::mem::take(&mut self.marks) {
                if !overlaps(&mark.range, &inserted_range) || kinds.contains(&mark.kind) {
                    kept.push(mark);
                    continue;
                }
                if mark.range.start < inserted_range.start {
                    kept.push(Mark {
                        range: mark.range.start..inserted_range.start,
                        kind: mark.kind.clone(),
                    });
                }
                if mark.range.end > inserted_range.end {
                    kept.push(Mark {
                        range: inserted_range.end..mark.range.end,
                        kind: mark.kind,
                    });
                }
            }
            kept.extend(kinds.iter().map(|kind| Mark {
                range: inserted_range.clone(),
                kind: kind.clone(),
            }));
            self.marks = kept;
        }
        self.normalize_marks();
        self.journal.record(range, inserted, &self.text);
    }

    fn previous_number(&self, index: usize, depth: u8) -> Option<u32> {
        let ranges = self.line_ranges();
        for previous in (0..index).rev() {
            match &self.lines[previous] {
                LineKind::Numbered(previous_depth) if *previous_depth == depth => {
                    return numbered_marker(&self.text[ranges[previous].clone()])
                        .map(|(_, number)| number);
                }
                kind if kind.is_list() && kind.depth() > depth => continue,
                _ => return None,
            }
        }
        None
    }

    fn expected_marker(&self, index: usize, own_number: Option<u32>) -> String {
        let kind = &self.lines[index];
        let number = match kind {
            LineKind::Numbered(depth) => match self.previous_number(index, *depth) {
                Some(previous) => previous + 1,
                None => own_number
                    .or_else(|| {
                        let line = self.line_ranges()[index].clone();
                        numbered_marker(&self.text[line]).map(|(_, number)| number)
                    })
                    .unwrap_or(1),
            },
            _ => 1,
        };
        marker_text(kind, number)
    }

    fn set_line_kind(
        &mut self,
        index: usize,
        kind: LineKind,
        number: Option<u32>,
        cursor: usize,
    ) -> usize {
        let marker = self.marker_range(index);
        self.lines[index] = kind;
        let expected = self.expected_marker(index, number);
        self.replace_marker(marker, &expected, cursor)
    }

    fn replace_marker(&mut self, marker: Range<usize>, expected: &str, cursor: usize) -> usize {
        if self.text[marker.clone()] == *expected {
            return cursor;
        }
        self.splice(marker.clone(), expected, &[]);
        if cursor >= marker.end {
            cursor + expected.len() - marker.len()
        } else if cursor > marker.start {
            marker.start + expected.len()
        } else {
            cursor
        }
    }

    /// List items whose marker was edited away become text; numbered items are renumbered.
    fn repair(&mut self, mut cursor: usize) -> usize {
        for index in 0..self.lines.len() {
            let kind = self.lines[index].clone();
            if !kind.is_list() {
                continue;
            }
            let line = self.line_ranges()[index].clone();
            let expected = self.expected_marker(index, None);
            let current = &self.text[line.clone()];
            if current.starts_with(&expected) {
                continue;
            }
            match numbered_marker(current) {
                Some((length, _)) if matches!(kind, LineKind::Numbered(_)) => {
                    cursor =
                        self.replace_marker(line.start..line.start + length, &expected, cursor);
                }
                _ => self.lines[index] = LineKind::Text,
            }
        }
        cursor
    }
}

fn overlaps(left: &Range<usize>, right: &Range<usize>) -> bool {
    left.start < right.end && right.start < left.end
}

fn bullet_marker(depth: u8) -> String {
    format!("{} ", BULLETS[depth as usize % BULLETS.len()])
}

fn marker_text(kind: &LineKind, number: u32) -> String {
    match kind {
        LineKind::Bullet(depth) => bullet_marker(*depth),
        LineKind::Numbered(_) => format!("{number}. "),
        LineKind::Text | LineKind::Quote | LineKind::Code(_) => String::new(),
    }
}

fn marker_len(kind: &LineKind, line: &str) -> usize {
    match kind {
        LineKind::Bullet(depth) => {
            let marker = bullet_marker(*depth);
            if line.starts_with(&marker) {
                marker.len()
            } else {
                0
            }
        }
        LineKind::Numbered(_) => numbered_marker(line).map_or(0, |(length, _)| length),
        _ => 0,
    }
}

/// Byte length and number of a `1. ` marker.
fn numbered_marker(line: &str) -> Option<(usize, u32)> {
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > 9 || !line[digits..].starts_with(". ") {
        return None;
    }
    Some((digits + 2, line[..digits].parse().ok()?))
}

fn line_shortcut(before: &str) -> Option<(LineKind, u32)> {
    match before {
        "- " | "* " => Some((LineKind::Bullet(0), 1)),
        "> " => Some((LineKind::Quote, 1)),
        _ => {
            let digits = before.strip_suffix(". ")?;
            let all_digits = !digits.is_empty()
                && digits.len() <= 9
                && digits.bytes().all(|byte| byte.is_ascii_digit());
            all_digits.then(|| (LineKind::Numbered(0), digits.parse().unwrap_or(1)))
        }
    }
}

fn fence_language(line: &str) -> Option<Option<String>> {
    let info = line.trim().strip_prefix(FENCE)?;
    let valid = info
        .chars()
        .all(|character| character.is_alphanumeric() || "+#-_.".contains(character));
    valid.then(|| (!info.is_empty()).then(|| info.to_lowercase()))
}

fn valid_inner(inner: &str) -> bool {
    !inner.is_empty()
        && !inner.starts_with(char::is_whitespace)
        && !inner.ends_with(char::is_whitespace)
}

fn word_before(text: &str) -> bool {
    text.chars()
        .next_back()
        .is_some_and(|character| character.is_alphanumeric())
}

/// `https://` added when there is no scheme. `None` for any scheme but http, https and mailto.
pub fn link_url(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() || text.contains(char::is_whitespace) {
        return None;
    }
    let lower = text.to_ascii_lowercase();
    if LINK_SCHEMES.iter().any(|scheme| lower.starts_with(scheme)) {
        return Some(text.to_owned());
    }
    let scheme = text.split_once(':').filter(|(scheme, rest)| {
        let host = scheme.contains('.') || scheme.eq_ignore_ascii_case("localhost");
        let host_and_port = host && rest.starts_with(|character: char| character.is_ascii_digit());
        !host_and_port
            && scheme.starts_with(|character: char| character.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "+.-".contains(character))
    });
    scheme.is_none().then(|| format!("https://{text}"))
}

fn autolink_prefix(text: &str) -> Option<&'static str> {
    AUTOLINK_PREFIXES.into_iter().find(|prefix| {
        text.get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
    })
}

/// Length of the URL at the start of `rest`, when `previous` does not continue a word.
fn bare_url_at(rest: &str, previous: Option<char>) -> Option<usize> {
    if previous.is_some_and(char::is_alphanumeric) {
        return None;
    }
    bare_url_len(rest)
}

/// Length of the URL a word starts with, without trailing punctuation.
fn bare_url_len(rest: &str) -> Option<usize> {
    let word = &rest[..rest.find(char::is_whitespace).unwrap_or(rest.len())];
    let prefix = autolink_prefix(word)?;
    let mut end = word.len();
    while let Some(last) = word[..end].chars().next_back() {
        let trailing = match last {
            ')' => word[..end].matches(')').count() > word[..end].matches('(').count(),
            other => AUTOLINK_TRAILING.contains(other),
        };
        if !trailing {
            break;
        }
        end -= last.len_utf8();
    }
    (end > prefix.len()).then_some(end)
}

/// The markers of a format closed by the last typed character, relative to `before`.
fn typed_inline(before: &str) -> Option<(Range<usize>, Range<usize>, MarkKind)> {
    let end = before.len();
    if before.ends_with(')') {
        let captures = TYPED_LINK.captures(before)?;
        let whole = captures.get(0)?;
        let label = captures.get(1)?;
        return Some((
            whole.start()..whole.start() + 1,
            label.end()..end,
            MarkKind::Link(link_url(&captures[2])?),
        ));
    }
    for (marker, kind) in [("**", MarkKind::Bold), ("~~", MarkKind::Strike)] {
        if let Some(body) = before.strip_suffix(marker) {
            let open = body.rfind(marker)?;
            return valid_inner(&body[open + 2..]).then(|| (open..open + 2, body.len()..end, kind));
        }
    }
    if let Some(body) = before.strip_suffix('`') {
        let open = body.rfind('`')?;
        let usable = !body[open + 1..].trim().is_empty() && !body[..open].ends_with('`');
        return usable.then(|| (open..open + 1, body.len()..end, MarkKind::Code));
    }
    for marker in ['*', '_'] {
        if let Some(body) = before.strip_suffix(marker) {
            if body.ends_with(marker) {
                return None;
            }
            let open = body.rfind(marker)?;
            let opening_ok = !word_before(&body[..open]) && !body[..open].ends_with(marker);
            return (opening_ok && valid_inner(&body[open + 1..]))
                .then(|| (open..open + 1, body.len()..end, MarkKind::Italic));
        }
    }
    None
}

pub fn changed_span(old: &str, new: &str) -> (usize, usize, usize) {
    edit_span(old, new, new.len())
}

fn edit_span(old: &str, new: &str, cursor: usize) -> (usize, usize, usize) {
    let prefix = old
        .char_indices()
        .zip(new.chars())
        .find(|((_, left), right)| left != right)
        .map_or(old.len().min(new.len()), |((index, _), _)| index);
    let prefix = {
        let mut prefix = prefix.min(old.len()).min(new.len());
        while !old.is_char_boundary(prefix) || !new.is_char_boundary(prefix) {
            prefix -= 1;
        }
        prefix
    };
    let suffix = old[prefix..]
        .chars()
        .rev()
        .zip(new[prefix..].chars().rev())
        .take_while(|(left, right)| left == right)
        .map(|(character, _)| character.len_utf8())
        .sum::<usize>();
    let (start, old_end, new_end) = (prefix, old.len() - suffix, new.len() - suffix);
    let cursor = cursor.min(new.len());
    let pure = old_end == start || new_end == start;
    if pure && new_end > cursor && new_end - cursor <= start {
        let slide = new_end - cursor;
        let (slid_start, slid_old_end, slid_new_end) =
            (start - slide, old_end - slide, new_end - slide);
        let boundaries = [slid_start, slid_old_end]
            .iter()
            .all(|offset| old.is_char_boundary(*offset))
            && [slid_start, slid_new_end]
                .iter()
                .all(|offset| new.is_char_boundary(*offset));
        if boundaries && new[slid_new_end..] == old[slid_old_end..] {
            return (slid_start, slid_old_end, slid_new_end);
        }
    }
    (start, old_end, new_end)
}

fn joined_inline(lines: &[DraftLine]) -> String {
    lines
        .iter()
        .map(|line| inline_html(&line.content, &line.marks))
        .collect::<Vec<_>>()
        .join("<br>")
}

fn inline_html(content: &str, marks: &[Mark]) -> String {
    let mut boundaries: Vec<usize> = marks
        .iter()
        .flat_map(|mark| [mark.range.start, mark.range.end])
        .chain([0, content.len()])
        .filter(|offset| *offset <= content.len())
        .collect();
    boundaries.sort_unstable();
    boundaries.dedup();
    let mut html = String::new();
    let mut open: Vec<&MarkKind> = Vec::new();
    for window in boundaries.windows(2) {
        let segment = window[0]..window[1];
        let mut active: Vec<&MarkKind> = marks
            .iter()
            .filter(|mark| mark.range.start <= segment.start && segment.end <= mark.range.end)
            .map(|mark| &mark.kind)
            .collect();
        active.sort();
        active.dedup();
        let common = open
            .iter()
            .zip(active.iter())
            .take_while(|(left, right)| left == right)
            .count();
        for kind in open.drain(common..).rev() {
            html.push_str(kind.close_tag());
        }
        for kind in &active[common..] {
            html.push_str(&kind.open_tag());
        }
        open.extend(active[common..].iter().copied());
        html.push_str(&escape_html(&content[segment]));
    }
    for kind in open.into_iter().rev() {
        html.push_str(kind.close_tag());
    }
    html
}

fn list_html(lines: &[DraftLine]) -> String {
    let close = |ordered: bool| if ordered { "</li></ol>" } else { "</li></ul>" };
    let mut html = String::new();
    let mut open: Vec<(u8, bool)> = Vec::new();
    for line in lines {
        let ordered = matches!(line.kind, LineKind::Numbered(_));
        let depth = line
            .kind
            .depth()
            .min(open.last().map_or(0, |(depth, _)| depth + 1));
        while open
            .last()
            .is_some_and(|(open_depth, _)| *open_depth > depth)
        {
            let (_, was_ordered) = open.pop().unwrap_or_default();
            html.push_str(close(was_ordered));
        }
        match open.last().copied() {
            Some((open_depth, open_ordered)) if open_depth == depth && open_ordered == ordered => {
                html.push_str("</li>");
            }
            Some((open_depth, open_ordered)) if open_depth == depth => {
                html.push_str(close(open_ordered));
                open.pop();
                html.push_str(&list_open(ordered, line.number));
                open.push((depth, ordered));
            }
            _ => {
                html.push_str(&list_open(ordered, line.number));
                open.push((depth, ordered));
            }
        }
        html.push_str("<li>");
        html.push_str(&inline_html(&line.content, &line.marks));
    }
    for (_, ordered) in open.into_iter().rev() {
        html.push_str(close(ordered));
    }
    html
}

fn list_open(ordered: bool, number: Option<u32>) -> String {
    match (ordered, number) {
        (false, _) => "<ul>".to_owned(),
        (true, Some(start)) if start != 1 => format!("<ol start=\"{start}\">"),
        (true, _) => "<ol>".to_owned(),
    }
}

/// Text that `from_markdown` would format: pasting it converts.
pub fn has_markdown(text: &str) -> bool {
    let draft = Draft::from_markdown(text);
    !draft.marks.is_empty() || draft.lines.iter().any(|kind| *kind != LineKind::Text)
}

fn markdown_lines(text: &str) -> Vec<DraftLine> {
    let mut lines = Vec::new();
    let mut code: Option<Option<String>> = None;
    for raw in text.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        let trimmed = raw.trim_start();
        if let Some(language) = &code {
            if trimmed.starts_with(FENCE) && trimmed.trim_end() == FENCE {
                code = None;
            } else {
                lines.push(DraftLine {
                    kind: LineKind::Code(language.clone()),
                    number: None,
                    content: raw.to_owned(),
                    marks: Vec::new(),
                });
            }
            continue;
        }
        if trimmed.starts_with(FENCE)
            && !trimmed[FENCE.len()..].contains('`')
            && let Some(language) = fence_language(trimmed)
        {
            code = Some(language);
            continue;
        }
        let indent: usize = raw[..raw.len() - trimmed.len()]
            .chars()
            .map(|character| if character == '\t' { 4 } else { 1 })
            .sum();
        let depth = ((indent / MARKDOWN_INDENT) as u8).min(MAX_DEPTH);
        let (kind, number, content) = if let Some(rest) = PASTED_BULLETS
            .iter()
            .find_map(|bullet| trimmed.strip_prefix(bullet))
        {
            (LineKind::Bullet(depth), None, rest)
        } else if let Some((length, number)) = numbered_marker(trimmed) {
            (LineKind::Numbered(depth), Some(number), &trimmed[length..])
        } else if let Some(rest) = trimmed
            .strip_prefix("> ")
            .or_else(|| (trimmed == ">").then_some(""))
        {
            (LineKind::Quote, None, rest)
        } else {
            (LineKind::Text, None, raw)
        };
        let (content, marks) = parse_inline(content);
        lines.push(DraftLine {
            kind,
            number,
            content,
            marks,
        });
    }
    lines
}

fn parse_inline(source: &str) -> (String, Vec<Mark>) {
    let mut text = String::with_capacity(source.len());
    let mut marks = Vec::new();
    parse_into(source, &mut text, &mut marks, true);
    (text, marks)
}

fn parse_into(source: &str, text: &mut String, marks: &mut Vec<Mark>, autolink: bool) {
    let mut rest = source;
    let mut previous: Option<char> = None;
    while let Some(character) = rest.chars().next() {
        if autolink
            && let Some(length) = bare_url_at(rest, previous)
            && let Some(url) = link_url(&rest[..length])
        {
            let start = text.len();
            text.push_str(&rest[..length]);
            marks.push(Mark {
                range: start..text.len(),
                kind: MarkKind::Link(url),
            });
            previous = rest[..length].chars().next_back();
            rest = &rest[length..];
            continue;
        }
        if let Some((inner, consumed, kind)) = inline_at(rest, previous) {
            let start = text.len();
            match kind {
                MarkKind::Code => text.push_str(inner),
                MarkKind::Link(_) => parse_into(inner, text, marks, false),
                _ => parse_into(inner, text, marks, autolink),
            }
            marks.push(Mark {
                range: start..text.len(),
                kind,
            });
            previous = rest[..consumed].chars().next_back();
            rest = &rest[consumed..];
            continue;
        }
        text.push(character);
        previous = Some(character);
        rest = &rest[character.len_utf8()..];
    }
}

/// A format starting at the beginning of `rest`: its inner text, the bytes it spans, its kind.
fn inline_at(rest: &str, previous: Option<char>) -> Option<(&str, usize, MarkKind)> {
    if let Some(after) = rest.strip_prefix('`') {
        let end = after.find('`')?;
        let inner = &after[..end];
        return (!inner.trim().is_empty()).then_some((inner, end + 2, MarkKind::Code));
    }
    if rest.starts_with('[') {
        let captures = LINK_AT.captures(rest)?;
        let label = captures.get(1)?;
        return Some((
            label.as_str(),
            captures.get(0)?.end(),
            MarkKind::Link(link_url(&captures[2])?),
        ));
    }
    for (marker, kind) in [("**", MarkKind::Bold), ("~~", MarkKind::Strike)] {
        if let Some(after) = rest.strip_prefix(marker) {
            let end = after.find(marker)?;
            let inner = &after[..end];
            return valid_inner(inner).then_some((inner, end + 4, kind));
        }
    }
    for marker in ['*', '_'] {
        let Some(after) = rest.strip_prefix(marker) else {
            continue;
        };
        if previous.is_some_and(char::is_alphanumeric) || after.starts_with(marker) {
            return None;
        }
        let end = after.find(marker)?;
        let inner = &after[..end];
        let followed_by_word = after[end + 1..]
            .chars()
            .next()
            .is_some_and(char::is_alphanumeric);
        return (valid_inner(inner) && !followed_by_word).then_some((
            inner,
            end + 2,
            MarkKind::Italic,
        ));
    }
    None
}

#[derive(Default)]
struct SpanLines {
    lines: Vec<DraftLine>,
    open: bool,
}

impl SpanLines {
    fn start_line(&mut self, kind: &LineKind, number: Option<u32>) {
        self.lines.push(DraftLine {
            kind: kind.clone(),
            number,
            content: String::new(),
            marks: Vec::new(),
        });
        self.open = true;
    }

    fn append(&mut self, text: &str, kind: &LineKind, marks: &[MarkKind]) {
        for (index, piece) in text.split('\n').enumerate() {
            if index > 0 || !self.open {
                self.start_line(kind, None);
            }
            let Some(line) = self.lines.last_mut() else {
                continue;
            };
            let start = line.content.len();
            line.content.push_str(piece);
            let end = line.content.len();
            if end > start {
                line.marks.extend(marks.iter().map(|kind| Mark {
                    range: start..end,
                    kind: kind.clone(),
                }));
            }
        }
    }

    fn walk(&mut self, spans: &[Span], kind: &LineKind, marks: &[MarkKind]) {
        let with = |extra: MarkKind| {
            let mut marks = marks.to_vec();
            marks.push(extra);
            marks
        };
        for (index, span) in spans.iter().enumerate() {
            let block_follows = matches!(
                spans.get(index + 1),
                Some(
                    Span::List { .. }
                        | Span::BlockQuote(_)
                        | Span::CodeBlock { .. }
                        | Span::Heading { .. }
                        | Span::Table { .. }
                )
            );
            match span {
                Span::Text(text) => self.append(text, kind, marks),
                Span::Bold(children) => self.walk(children, kind, &with(MarkKind::Bold)),
                Span::Italic(children) => self.walk(children, kind, &with(MarkKind::Italic)),
                Span::Strike(children) => self.walk(children, kind, &with(MarkKind::Strike)),
                Span::Underline(children) => self.walk(children, kind, &with(MarkKind::Underline)),
                Span::Colored { children, .. } => self.walk(children, kind, marks),
                Span::Code(text) => self.append(text, kind, &with(MarkKind::Code)),
                Span::Link { url, children } => {
                    self.walk(children, kind, &with(MarkKind::Link(url.clone())))
                }
                Span::Mention { name, .. } => {
                    self.append(&format!("@{}", name.trim_start_matches('@')), kind, marks)
                }
                Span::LineBreak | Span::Rule if !self.open || block_follows => {}
                Span::LineBreak | Span::Rule => {
                    let continued = match kind {
                        LineKind::Quote | LineKind::Code(_) => kind.clone(),
                        _ => LineKind::Text,
                    };
                    self.start_line(&continued, None);
                }
                Span::Heading { children, .. } => {
                    self.start_line(&LineKind::Text, None);
                    self.walk(children, &LineKind::Text, &with(MarkKind::Bold));
                    self.open = false;
                }
                Span::CodeBlock { language, code } => {
                    let code_kind = LineKind::Code(language.clone());
                    for line in code.trim_end_matches('\n').split('\n') {
                        self.start_line(&code_kind, None);
                        self.append(line, &code_kind, &[]);
                    }
                    self.open = false;
                }
                Span::BlockQuote(children) => {
                    self.start_line(&LineKind::Quote, None);
                    self.walk(children, &LineKind::Quote, marks);
                    self.open = false;
                }
                Span::List {
                    ordered,
                    start,
                    items,
                } => {
                    let depth = if kind.is_list() {
                        (kind.depth() + 1).min(MAX_DEPTH)
                    } else {
                        0
                    };
                    let item_kind = if *ordered {
                        LineKind::Numbered(depth)
                    } else {
                        LineKind::Bullet(depth)
                    };
                    for (position, item) in items.iter().enumerate() {
                        let number = (position == 0).then_some(*start);
                        self.start_line(&item_kind, number);
                        self.walk(item, &item_kind, marks);
                    }
                    self.open = false;
                }
                Span::Table { rows, .. } => {
                    for row in rows {
                        self.start_line(&LineKind::Text, None);
                        for (position, cell) in row.iter().enumerate() {
                            if position > 0 {
                                self.append(" | ", &LineKind::Text, &[]);
                            }
                            self.walk(cell, &LineKind::Text, marks);
                        }
                    }
                    self.open = false;
                }
                Span::Image { .. } | Span::Quote(_) => {}
            }
        }
    }

    fn finish(mut self) -> Vec<DraftLine> {
        let blank =
            |line: &DraftLine| line.kind == LineKind::Text && line.content.trim().is_empty();
        while self.lines.len() > 1 && self.lines.last().is_some_and(blank) {
            self.lines.pop();
        }
        while self.lines.len() > 1 && self.lines.first().is_some_and(blank) {
            self.lines.remove(0);
        }
        if self.lines.is_empty() {
            self.lines.push(DraftLine::text(""));
        }
        self.lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::html_to_spans;

    fn typed(draft: &mut Draft, text: &str) {
        let mut typing: Option<TypingStyle> = None;
        for character in text.chars() {
            let cursor = draft.text().len();
            let mut next = draft.text().to_owned();
            next.push(character);
            let cursor = draft.apply_edit(&next, cursor + character.len_utf8(), typing.as_ref());
            typing = draft.convert_typed(cursor).map(|(_, style)| style);
            draft.take_edits();
        }
    }

    #[test]
    fn object_marks_stay_in_the_html_and_leave_the_plain_text() {
        let draft = Draft::plain("a \u{FFFC} b");
        assert_eq!(draft.plain_text(), "a b");
        assert_eq!(draft.to_html(), "a \u{FFFC} b");
        assert!(Draft::plain("\u{FFFC}").is_blank());
    }

    #[test]
    fn slices_without_objects_keep_the_formatting_of_the_rest() {
        let mut draft = Draft::plain("ab\u{FFFC}cd");
        draft.toggle(0..draft.text().len(), MarkKind::Bold);
        let lines = draft.slice_without_objects(0..draft.text().len());
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].content, "abcd");
        assert_eq!(
            lines[0].marks,
            [Mark {
                range: 0..4,
                kind: MarkKind::Bold
            }]
        );
    }

    fn typed_draft(text: &str) -> Draft {
        let mut draft = Draft::default();
        typed(&mut draft, text);
        draft
    }

    fn mark(range: Range<usize>, kind: MarkKind) -> Mark {
        Mark { range, kind }
    }

    #[test]
    fn typed_markdown_becomes_formatting() {
        let draft = typed_draft("a **bold** and *it* or _it_ ~~gone~~ `code`");
        assert_eq!(draft.text(), "a bold and it or it gone code");
        assert_eq!(
            draft.marks(),
            [
                mark(2..6, MarkKind::Bold),
                mark(11..13, MarkKind::Italic),
                mark(17..19, MarkKind::Italic),
                mark(20..24, MarkKind::Strike),
                mark(25..29, MarkKind::Code),
            ]
        );
    }

    #[test]
    fn typed_link_keeps_the_label_and_carries_the_url() {
        let draft = typed_draft("see [docs](https://example.com)");
        assert_eq!(draft.text(), "see docs");
        assert_eq!(
            draft.marks(),
            [mark(4..8, MarkKind::Link("https://example.com".into()))]
        );
    }

    fn typed_autolinked(draft: &mut Draft, text: &str) {
        for character in text.chars() {
            let mut next = draft.text().to_owned();
            next.push(character);
            let cursor = next.len();
            draft.apply_edit(&next, cursor, None);
            draft.autolink_typed(cursor);
            draft.take_edits();
        }
    }

    fn autolinked(text: &str) -> Draft {
        let mut draft = Draft::default();
        typed_autolinked(&mut draft, text);
        draft
    }

    fn edited(draft: &mut Draft, range: Range<usize>, replacement: &str) {
        let mut next = draft.text().to_owned();
        next.replace_range(range.clone(), replacement);
        draft.apply_edit(&next, range.start + replacement.len(), None);
    }

    #[test]
    fn typed_url_followed_by_a_space_becomes_a_link() {
        let draft = autolinked("https://example.com ");
        assert_eq!(
            draft.marks(),
            [mark(0..19, MarkKind::Link("https://example.com".into()))]
        );
    }

    #[test]
    fn typed_url_followed_by_a_newline_becomes_a_link() {
        let draft = autolinked("a mailto:x@y.de\n");
        assert_eq!(
            draft.marks(),
            [mark(2..15, MarkKind::Link("mailto:x@y.de".into()))]
        );
    }

    #[test]
    fn autolink_leaves_trailing_punctuation_out_and_adds_the_scheme() {
        let draft = autolinked("see www.x.de. ");
        assert_eq!(
            draft.marks(),
            [mark(4..12, MarkKind::Link("https://www.x.de".into()))]
        );
        let wiki = autolinked("https://a.b/x_(y) ");
        assert_eq!(
            wiki.marks(),
            [mark(0..17, MarkKind::Link("https://a.b/x_(y)".into()))]
        );
        let bracketed = autolinked("https://a.b) ");
        assert_eq!(
            bracketed.marks(),
            [mark(0..11, MarkKind::Link("https://a.b".into()))]
        );
    }

    #[test]
    fn autolink_starts_after_any_non_alphanumeric_character() {
        let bracketed = autolinked("(https://a.b) ");
        assert_eq!(
            bracketed.marks(),
            [mark(1..12, MarkKind::Link("https://a.b".into()))]
        );
        let quoted = autolinked("\"www.x.de\" ");
        assert_eq!(
            quoted.marks(),
            [mark(1..9, MarkKind::Link("https://www.x.de".into()))]
        );
        let colon = autolinked("see:https://a.b ");
        assert_eq!(
            colon.marks(),
            [mark(4..15, MarkKind::Link("https://a.b".into()))]
        );
        assert!(autolinked("xhttps://a.b ").marks().is_empty());
    }

    #[test]
    fn a_link_labelled_with_a_bare_domain_keeps_its_url_when_relabelled() {
        let mut draft = Draft::plain("example.com");
        draft.set_link(0..11, "example.com");
        edited(&mut draft, 0..11, "our site");
        assert_eq!(
            draft.marks(),
            [mark(0..8, MarkKind::Link("https://example.com".into()))]
        );
    }

    #[test]
    fn autolink_skips_plain_words_code_and_existing_links() {
        assert!(autolinked("example.com ").marks().is_empty());
        assert!(autolinked("http:// ").marks().is_empty());
        let mut block = Draft::from_markdown("```\nhttps://a.b");
        typed_autolinked(&mut block, " ");
        assert!(block.marks().is_empty());
        let mut code = Draft::plain("https://a.b");
        code.toggle(0..11, MarkKind::Code);
        typed_autolinked(&mut code, " ");
        assert_eq!(code.marks(), [mark(0..11, MarkKind::Code)]);
        let mut linked = Draft::plain("https://a.b");
        linked.set_link(0..11, "https://other.de");
        typed_autolinked(&mut linked, " ");
        assert_eq!(
            linked.marks(),
            [mark(0..11, MarkKind::Link("https://other.de".into()))]
        );
    }

    #[test]
    fn markdown_import_links_bare_urls_outside_code_and_labels() {
        let draft = Draft::from_markdown("go to https://a.b now");
        assert_eq!(
            draft.marks(),
            [mark(6..17, MarkKind::Link("https://a.b".into()))]
        );
        assert_eq!(
            Draft::from_markdown("run `https://a.b` now").marks(),
            [mark(4..15, MarkKind::Code)]
        );
        let labelled = Draft::from_markdown("[https://a.b](https://c.d)");
        assert_eq!(
            labelled.marks(),
            [mark(0..11, MarkKind::Link("https://c.d".into()))]
        );
        assert!(has_markdown("x https://a.b y"));
    }

    #[test]
    fn editing_an_auto_link_moves_its_url() {
        let mut draft = autolinked("https://a.bc ");
        edited(&mut draft, 11..12, "d");
        assert_eq!(
            draft.marks(),
            [mark(0..12, MarkKind::Link("https://a.bd".into()))]
        );
        edited(&mut draft, 11..11, "e");
        assert_eq!(
            draft.marks(),
            [mark(0..13, MarkKind::Link("https://a.bed".into()))]
        );
    }

    #[test]
    fn editing_a_labelled_link_keeps_its_url() {
        let mut draft = typed_draft("[docs](https://example.com)");
        edited(&mut draft, 2..2, "x");
        assert_eq!(
            draft.marks(),
            [mark(0..5, MarkKind::Link("https://example.com".into()))]
        );
    }

    #[test]
    fn breaking_an_auto_link_into_non_url_text_removes_it() {
        let mut draft = autolinked("https://a.bc ");
        edited(&mut draft, 9..9, " ");
        assert!(draft.marks().is_empty());
    }

    #[test]
    fn an_auto_link_can_be_relabelled_or_unlinked() {
        let mut draft = autolinked("https://a.bc ");
        assert_eq!(draft.link_in(0..4), Some("https://a.bc".into()));
        draft.set_link(0..12, "https://other.de");
        assert_eq!(
            draft.marks(),
            [mark(0..12, MarkKind::Link("https://other.de".into()))]
        );
        edited(&mut draft, 12..12, "x");
        draft.set_link(0..12, "");
        assert!(draft.marks().is_empty());
    }

    #[test]
    fn snake_case_and_spaced_stars_stay_text() {
        assert_eq!(typed_draft("snake_case_name").text(), "snake_case_name");
        assert_eq!(typed_draft("2 * 3 * 4").text(), "2 * 3 * 4");
        assert!(typed_draft("a** b**").marks().is_empty());
    }

    #[test]
    fn typing_at_the_end_of_bold_continues_it_but_not_after_a_link() {
        let mut draft = Draft::plain("b");
        draft.toggle(0..1, MarkKind::Bold);
        typed(&mut draft, "x");
        assert_eq!(draft.marks(), [mark(0..2, MarkKind::Bold)]);
        let converted = typed_draft("**b**x");
        assert_eq!(converted.marks(), [mark(0..1, MarkKind::Bold)]);
        let mut linked = typed_draft("[a](u)");
        typed(&mut linked, "b");
        assert_eq!(
            linked.marks(),
            [mark(0..1, MarkKind::Link("https://u".into()))]
        );
    }

    #[test]
    fn typing_style_switches_bold_on_and_off_for_the_next_text() {
        let mut draft = Draft::plain("ab");
        let typing = TypingStyle {
            at: 2,
            add: vec![MarkKind::Bold],
            remove: Vec::new(),
        };
        draft.apply_edit("abc", 3, Some(&typing));
        assert_eq!(draft.marks(), [mark(2..3, MarkKind::Bold)]);
        let off = TypingStyle {
            at: 3,
            add: Vec::new(),
            remove: vec![MarkKind::Bold],
        };
        draft.apply_edit("abcd", 4, Some(&off));
        assert_eq!(draft.marks(), [mark(2..3, MarkKind::Bold)]);
    }

    #[test]
    fn toggle_on_a_mixed_selection_formats_all_then_clears() {
        let mut draft = Draft::plain("hello world");
        draft.toggle(0..5, MarkKind::Bold);
        assert_eq!(draft.state(0..11, &MarkKind::Bold), FormatState::Mixed);
        draft.toggle(0..11, MarkKind::Bold);
        assert_eq!(draft.state(0..11, &MarkKind::Bold), FormatState::On);
        draft.toggle(0..11, MarkKind::Bold);
        assert_eq!(draft.state(0..11, &MarkKind::Bold), FormatState::Off);
        assert!(draft.marks().is_empty());
    }

    #[test]
    fn deleting_inside_and_across_marks_shifts_and_clips() {
        let mut draft = Draft::plain("aa bold cc");
        draft.toggle(3..7, MarkKind::Bold);
        draft.apply_edit("aa bld cc", 4, None);
        assert_eq!(draft.marks(), [mark(3..6, MarkKind::Bold)]);
        draft.apply_edit("ald cc", 1, None);
        assert_eq!(draft.marks(), [mark(1..3, MarkKind::Bold)]);
        draft.apply_edit("XXald cc", 2, None);
        assert_eq!(draft.marks(), [mark(3..5, MarkKind::Bold)]);
    }

    #[test]
    fn repeated_characters_are_attributed_to_the_cursor() {
        let mut draft = Draft::plain("aa");
        draft.toggle(0..1, MarkKind::Bold);
        draft.apply_edit("aaa", 2, None);
        assert_eq!(draft.marks(), [mark(0..2, MarkKind::Bold)]);
    }

    #[test]
    fn list_shortcuts_turn_lines_into_items() {
        let draft = typed_draft("- one");
        assert_eq!(draft.text(), "• one");
        assert_eq!(draft.lines(), [LineKind::Bullet(0)]);
        let numbered = typed_draft("1. one");
        assert_eq!(numbered.text(), "1. one");
        assert_eq!(numbered.lines(), [LineKind::Numbered(0)]);
        let quote = typed_draft("> said");
        assert_eq!(quote.text(), "said");
        assert_eq!(quote.lines(), [LineKind::Quote]);
    }

    #[test]
    fn enter_continues_a_list_and_ends_it_on_an_empty_item() {
        let mut draft = typed_draft("1. one");
        let cursor = draft.break_line(draft.text().len(), false).unwrap();
        assert_eq!(draft.text(), "1. one\n2. ");
        assert_eq!(cursor, draft.text().len());
        typed(&mut draft, "two");
        let cursor = draft.break_line(draft.text().len(), false).unwrap();
        let cursor = draft.break_line(cursor, false).unwrap();
        assert_eq!(draft.text(), "1. one\n2. two\n");
        assert_eq!(cursor, draft.text().len());
        assert_eq!(draft.lines()[2], LineKind::Text);
        assert_eq!(Draft::plain("x").break_line(1, false), None);
    }

    #[test]
    fn enter_in_the_middle_of_an_item_splits_it_and_renumbers() {
        let mut draft = Draft::from_markdown("1. ab\n2. c");
        draft.break_line(4, false).unwrap();
        assert_eq!(draft.text(), "1. a\n2. b\n3. c");
        let edits = draft.take_edits();
        assert_eq!(map_offset(&edits, 4), 8);
    }

    #[test]
    fn tab_nests_and_shift_tab_lifts_list_items() {
        let mut draft = Draft::from_markdown("- a\n- b");
        assert!(draft.indent(6..6, false));
        assert_eq!(draft.lines(), [LineKind::Bullet(0), LineKind::Bullet(1)]);
        assert_eq!(draft.text(), "• a\n◦ b");
        assert!(draft.indent(draft.text().len()..draft.text().len(), true));
        assert!(draft.indent(draft.text().len()..draft.text().len(), true));
        assert_eq!(draft.text(), "• a\nb");
        assert!(!Draft::plain("x").indent(0..0, false));
    }

    #[test]
    fn nested_numbering_counts_per_level() {
        let draft = Draft::from_markdown("1. a\n  1. x\n  2. y\n2. b");
        assert_eq!(draft.text(), "1. a\n1. x\n2. y\n2. b");
    }

    #[test]
    fn backspace_at_an_item_start_drops_the_bullet() {
        let mut draft = Draft::from_markdown("- a");
        let cursor = draft.backspace_at_start(bullet_marker(0).len()).unwrap();
        assert_eq!((draft.text(), cursor), ("a", 0));
        assert_eq!(draft.lines(), [LineKind::Text]);
    }

    #[test]
    fn typing_before_a_bullet_lands_after_it() {
        let mut draft = Draft::from_markdown("- a");
        let marker = bullet_marker(0);
        let cursor = draft.apply_edit(&format!("x{marker}a"), 1, None);
        assert_eq!(draft.text(), format!("{marker}xa"));
        assert_eq!(cursor, marker.len() + 1);
        assert_eq!(
            draft.take_edits(),
            [
                (0..1, String::new()),
                (marker.len() + 1..marker.len() + 1, "x".to_owned())
            ]
        );
    }

    #[test]
    fn a_damaged_marker_turns_the_line_into_text() {
        let mut draft = Draft::from_markdown("- a");
        draft.apply_edit("•a", 1, None);
        assert_eq!(draft.lines(), [LineKind::Text]);
    }

    #[test]
    fn fence_and_enter_open_a_code_block_that_enter_continues() {
        let mut draft = typed_draft("```rust");
        let cursor = draft.break_line(draft.text().len(), false).unwrap();
        assert_eq!((draft.text(), cursor), ("", 0));
        assert_eq!(draft.lines(), [LineKind::Code(Some("rust".into()))]);
        typed(&mut draft, "fn x() {}");
        let cursor = draft.break_line(draft.text().len(), false).unwrap();
        assert_eq!(draft.lines().len(), 2);
        assert_eq!(draft.break_line(cursor, false), Some(cursor));
        assert_eq!(draft.lines()[1], LineKind::Text);
        assert_eq!(
            draft.to_html(),
            "<pre class=\"language-rust\">fn x() {}</pre>"
        );
    }

    #[test]
    fn markdown_inside_code_stays_literal() {
        let mut draft = typed_draft("```");
        let cursor = draft.break_line(3, false).unwrap();
        assert_eq!(cursor, 0);
        typed(&mut draft, "a **b**");
        assert_eq!(draft.text(), "a **b**");
        assert!(draft.marks().is_empty());
    }

    #[test]
    fn html_nests_overlapping_marks_and_escapes() {
        let mut draft = Draft::plain("a<b & c");
        draft.toggle(0..4, MarkKind::Bold);
        draft.toggle(2..7, MarkKind::Italic);
        assert_eq!(draft.to_html(), "<b>a&lt;<i>b </i></b><i>&amp; c</i>");
    }

    #[test]
    fn html_covers_every_format() {
        let draft = Draft::from_markdown(
            "**b** *i* ~~s~~ `c` [l](https://x.test/?a=1&b=2)\n> q1\n> q2\n- a\n  - b\n1. x\n2. y\n```py\nprint(1)\n```",
        );
        let mut underlined = draft.clone();
        underlined.toggle(0..1, MarkKind::Underline);
        assert_eq!(
            underlined.to_html(),
            "<b><u>b</u></b> <i>i</i> <s>s</s> <code>c</code> <a href=\"https://x.test/?a=1&amp;b=2\">l</a>\
             <blockquote>q1<br>q2</blockquote>\
             <ul><li>a<ul><li>b</li></ul></li></ul>\
             <ol><li>x</li><li>y</li></ol>\
             <pre class=\"language-py\">print(1)</pre>"
        );
    }

    #[test]
    fn ordered_lists_keep_their_start() {
        let draft = Draft::from_markdown("3. c\n4. d");
        assert_eq!(draft.to_html(), "<ol start=\"3\"><li>c</li><li>d</li></ol>");
    }

    #[test]
    fn spans_round_trip_through_html() {
        let source = Draft::from_markdown(
            "hi **bold _both_** `c` [l](https://x.test)\n> quoted\n- a\n  - b\n2. two\n3. three\n```rs\nlet x = 1;\n```\nend",
        );
        let back = Draft::from_spans(&html_to_spans(&source.to_html()));
        assert_eq!(back.text(), source.text());
        assert_eq!(back.lines(), source.lines());
        assert_eq!(back.marks(), source.marks());
    }

    #[test]
    fn mentions_from_html_come_back_as_at_text() {
        let draft = Draft::from_spans(&html_to_spans("hi <at id=\"0\">Ada</at> <b>now</b>"));
        assert_eq!(draft.text(), "hi @Ada now");
        assert_eq!(draft.marks(), [mark(8..11, MarkKind::Bold)]);
    }

    #[test]
    fn pasted_markdown_is_detected() {
        assert!(has_markdown("see **this**"));
        assert!(has_markdown("- a\n- b"));
        assert!(has_markdown("```\nx\n```"));
        assert!(!has_markdown("plain text, 2*3=6 and snake_case"));
    }

    #[test]
    fn slices_carry_formatting_into_another_draft() {
        let source = Draft::from_markdown("intro **bold**\n- one\n- two");
        let end = source.text().len();
        let fragment = source.slice(6..end);
        assert_eq!(fragment[0].kind, LineKind::Text);
        assert_eq!(fragment[1].kind, LineKind::Bullet(0));
        let mut target = Draft::plain("x ");
        let cursor = target.insert_lines(2..2, &fragment);
        assert_eq!(target.text(), "x bold\n• one\n• two");
        assert_eq!(cursor, target.text().len());
        assert_eq!(target.marks(), [mark(2..6, MarkKind::Bold)]);
        assert_eq!(
            target.lines()[1..],
            [LineKind::Bullet(0), LineKind::Bullet(0)]
        );
    }

    #[test]
    fn markdown_pasted_on_its_own_line_converts_from_the_first_line() {
        let raw = "- a\n- b";
        let mut draft = Draft::plain("");
        draft.apply_edit(raw, raw.len(), None);
        let lines = Draft::from_markdown(raw).slice(0..usize::MAX);
        let cursor = draft.insert_lines(0..raw.len(), &lines);
        assert_eq!(draft.text(), "• a\n• b");
        assert_eq!(cursor, draft.text().len());
        let mut inline = Draft::plain("x ");
        inline.apply_edit("x **b**", 7, None);
        let lines = Draft::from_markdown("**b**").slice(0..usize::MAX);
        inline.insert_lines(2..7, &lines);
        assert_eq!(inline.text(), "x b");
        assert_eq!(inline.marks(), [mark(2..3, MarkKind::Bold)]);
    }

    #[test]
    fn a_paste_inside_a_line_puts_the_cursor_after_the_pasted_text() {
        let lines = Draft::from_markdown("**big**").slice(0..usize::MAX);
        let mut plain = Draft::plain("hello world");
        assert_eq!(plain.insert_lines(6..6, &lines), 9);
        assert_eq!(plain.text(), "hello bigworld");
        let mut item = Draft::from_markdown("- item one");
        let before_one = bullet_marker(0).len() + "item ".len();
        assert_eq!(
            item.insert_lines(before_one..before_one, &lines),
            before_one + 3
        );
    }

    #[test]
    fn a_selection_ending_at_the_next_line_start_leaves_that_line_alone() {
        let mut draft = Draft::plain("first\nsecond");
        assert_eq!(
            draft.line_state(0..6, &LineKind::Bullet(0)),
            FormatState::Off
        );
        draft.toggle_lines(0..6, LineKind::Bullet(0));
        assert_eq!(draft.lines(), [LineKind::Bullet(0), LineKind::Text]);
        let end_of_first = draft.line_ranges()[1].start;
        assert_eq!(
            draft.line_state(0..end_of_first, &LineKind::Bullet(0)),
            FormatState::On
        );
        assert!(draft.indent(0..end_of_first, false));
        assert_eq!(draft.lines()[1], LineKind::Text);
    }

    #[test]
    fn links_only_take_http_https_and_mailto() {
        assert_eq!(link_url("example.com"), Some("https://example.com".into()));
        assert_eq!(link_url("mailto:a@b.test"), Some("mailto:a@b.test".into()));
        assert_eq!(link_url("javascript:alert(1)"), None);
        assert_eq!(link_url("data:text/html,x"), None);
        let typed = typed_draft("[x](javascript:alert)");
        assert!(typed.marks().is_empty());
        let pasted = Draft::from_markdown("[x](javascript:alert) [y](example.com)");
        assert_eq!(
            pasted.marks(),
            [mark(22..23, MarkKind::Link("https://example.com".into()))]
        );
        let mut set = Draft::plain("go");
        set.set_link(0..2, "javascript:alert(1)");
        assert!(set.marks().is_empty());
        let spans = Draft::from_spans(&html_to_spans("<a href=\"javascript:x\">bad</a>"));
        assert!(spans.marks().is_empty());
    }

    #[test]
    fn code_lines_hold_only_their_content() {
        let mut draft = Draft::from_markdown("```\nlet a = 1;\n\u{2003}b\n```");
        assert_eq!(draft.text(), "let a = 1;\n\u{2003}b");
        assert_eq!(draft.to_html(), "<pre>let a = 1;\n\u{2003}b</pre>");
        assert_eq!(draft.backspace_at_start(draft.line_ranges()[1].start), None);
        assert_eq!(draft.backspace_at_start(0), Some(0));
        assert_eq!(draft.lines()[0], LineKind::Text);
    }

    #[test]
    fn in_code_covers_code_lines_and_the_inside_of_inline_code() {
        let draft = Draft::from_markdown("a `cd` e\n```\nx\n```");
        assert!(!draft.in_code(2));
        assert!(draft.in_code(3));
        assert!(!draft.in_code(4));
        assert!(draft.in_code(draft.text().len()));
    }

    #[test]
    fn code_indentation_is_kept_on_paste_split_and_send() {
        let mut draft = typed_draft("```py");
        let cursor = draft.break_line(draft.text().len(), false).unwrap();
        let pasted = "def f():\n    return 1";
        let mut next = draft.text().to_owned();
        next.insert_str(cursor, pasted);
        draft.apply_edit(&next, cursor + pasted.len(), None);
        assert_eq!(
            draft.to_html(),
            "<pre class=\"language-py\">def f():\n    return 1</pre>"
        );
        let mut split = Draft::from_markdown("```\n   a    b\n```");
        let after_a = "   a".len();
        split.break_line(after_a, false).unwrap();
        assert_eq!(split.to_html(), "<pre>   a\n    b</pre>");
    }

    #[test]
    fn delete_at_a_line_end_joins_the_next_item_without_its_bullet() {
        let mut draft = Draft::from_markdown("- a\n- b");
        let end_of_first = draft.line_ranges()[0].end;
        assert_eq!(draft.delete_forward(end_of_first), Some(end_of_first));
        assert_eq!(draft.text(), "• ab");
        assert_eq!(draft.delete_forward(0), Some(bullet_marker(0).len()));
        assert_eq!(draft.text(), "• b");
        assert_eq!(Draft::plain("a\nb").delete_forward(1), None);
    }

    #[test]
    fn host_and_port_links_are_schemeless() {
        assert_eq!(
            link_url("localhost:3000"),
            Some("https://localhost:3000".into())
        );
        assert_eq!(
            link_url("example.com:8080/x"),
            Some("https://example.com:8080/x".into())
        );
        assert_eq!(link_url("javascript:alert(1)"), None);
        assert_eq!(link_url("tel:0123456"), None);
    }

    #[test]
    fn a_quote_stays_a_quote_when_split_or_joined() {
        let mut draft = Draft::from_markdown("> a\n> b");
        let second = draft.line_ranges()[1].start;
        assert_eq!(draft.break_line(second, true), Some(second + 1));
        assert_eq!(
            draft.lines(),
            [LineKind::Quote, LineKind::Quote, LineKind::Quote]
        );
        let mut joined = Draft::from_markdown("> a\n> b");
        let second = joined.line_ranges()[1].start;
        assert_eq!(joined.backspace_at_start(second), Some(second - 1));
        assert_eq!(
            (joined.text(), joined.lines()),
            ("ab", &[LineKind::Quote][..])
        );
        assert_eq!(joined.backspace_at_start(0), Some(0));
        assert_eq!(joined.lines(), [LineKind::Text]);
    }

    #[test]
    fn journal_merges_edits_into_ranges_of_the_old_text() {
        let mut draft = Draft::plain("a **b** c");
        draft.splice(5..7, "", &[]);
        draft.splice(2..4, "", &[]);
        draft.splice(0..0, "X", &[]);
        let edits = draft.take_edits();
        assert_eq!(
            edits,
            [
                (0..0, "X".to_owned()),
                (2..4, String::new()),
                (5..7, String::new())
            ]
        );
        assert_eq!(
            reverse_edits(&edits, "a **b** c")[1],
            (3..3, "**".to_owned())
        );
        let mut touching = Draft::plain("abc");
        touching.splice(1..2, "XY", &[]);
        touching.splice(3..3, "Z", &[]);
        assert_eq!(touching.take_edits(), [(1..2, "XYZ".to_owned())]);
    }

    #[test]
    fn trimming_drops_blank_lines_and_outer_spaces() {
        let mut draft = Draft::plain("\n  hi **x**  \n\n");
        draft.toggle(6..11, MarkKind::Bold);
        let trimmed = draft.trimmed();
        assert_eq!(trimmed.text(), "hi **x**");
        assert_eq!(trimmed.marks(), [mark(3..8, MarkKind::Bold)]);
    }

    #[test]
    fn toolbar_line_toggles_apply_to_every_selected_line() {
        let mut draft = Draft::plain("a\nb");
        draft.toggle_lines(0..3, LineKind::Numbered(0));
        assert_eq!(draft.text(), "1. a\n2. b");
        assert_eq!(
            draft.line_state(0..4, &LineKind::Numbered(0)),
            FormatState::On
        );
        draft.toggle_lines(0..9, LineKind::Bullet(0));
        assert_eq!(draft.text(), "• a\n• b");
        let end = draft.text().len();
        draft.toggle_lines(0..end, LineKind::Bullet(0));
        assert_eq!(draft.text(), "a\nb");
    }

    #[test]
    fn links_are_set_replaced_and_removed() {
        let mut draft = Draft::plain("go here");
        draft.set_link(3..7, "https://a.test");
        assert_eq!(draft.link_in(4..4), Some("https://a.test".into()));
        draft.set_link(3..7, "");
        assert!(draft.marks().is_empty());
        let end = draft.insert_link(7, " https://b.test", "https://b.test");
        assert_eq!(end, draft.text().len());
    }
}
