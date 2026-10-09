use crate::input::{DecoratedMode, EditorMode};
use std::{collections::BTreeMap, ops::Range};

use gpui::{
    App, Context, Font, FontStyle, FontWeight, HighlightStyle, Hsla, Pixels, SharedString,
    WeakEntity, px,
};
use ropey::Rope;
use sum_tree::Bias;

use super::{InputBaseState, RopeExt as _};

/// Geometric presentation for an editor range decoration.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RangeDecorationStyle {
    /// Fill the continuous visual range.
    Fill,
    /// Draw a continuous one-pixel frame around the visual range.
    #[default]
    Frame,
    /// A rounded box per visual row around the glyphs, e.g. an inline code pill.
    Pill,
    /// One rounded box across the full text width of every line whose start lies in
    /// `range.start..=range.end`, e.g. a code block background. The range may be empty.
    Block,
    /// A bar at the left edge of the lines a [`Self::Block`] would cover, e.g. a quote.
    Bar,
}

/// A geometric decoration over a UTF-8 byte range.
#[derive(Clone, Debug, PartialEq)]
pub struct RangeDecoration {
    range: Range<usize>,
    style: RangeDecorationStyle,
    color: Option<Hsla>,
    border: Option<Hsla>,
    radius: Pixels,
}

impl RangeDecoration {
    /// Create a frame using the editor foreground color.
    pub fn new(range: Range<usize>) -> Self {
        Self {
            range,
            style: RangeDecorationStyle::default(),
            color: None,
            border: None,
            radius: px(0.),
        }
    }

    /// The half-open UTF-8 byte range supplied to this decoration.
    pub fn range(&self) -> &Range<usize> {
        &self.range
    }

    /// The geometric paint style.
    pub fn style(&self) -> RangeDecorationStyle {
        self.style
    }

    /// An application-owned color override, or `None` for the editor fallback.
    pub fn color(&self) -> Option<Hsla> {
        self.color
    }

    /// Choose a fill or frame without changing text layout.
    pub fn with_style(mut self, style: RangeDecorationStyle) -> Self {
        self.style = style;
        self
    }

    /// Override the editor foreground fallback with an application-owned color.
    pub fn with_color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    /// A one-pixel border for [`RangeDecorationStyle::Pill`] and [`RangeDecorationStyle::Block`].
    pub fn with_border(mut self, color: Hsla) -> Self {
        self.border = Some(color);
        self
    }

    /// Corner radius for pills and blocks; the width of a [`RangeDecorationStyle::Bar`].
    pub fn with_radius(mut self, radius: Pixels) -> Self {
        self.radius = radius;
        self
    }

    pub fn border(&self) -> Option<Hsla> {
        self.border
    }

    pub fn radius(&self) -> Pixels {
        self.radius
    }
}

/// A presentation style applied to a UTF-8 byte range in an input.
///
/// This is the GPUI [`HighlightStyle`] counterpart of Monaco's
/// [`IModelDeltaDecoration`](https://microsoft.github.io/monaco-editor/typedoc/interfaces/editor_editor_api.editor.IModelDeltaDecoration.html).
#[derive(Clone, Debug, PartialEq)]
pub struct TextDecoration {
    pub range: Range<usize>,
    pub style: HighlightStyle,
    /// Font family override. It, `style.font_weight` and `style.font_style` change soft wrapping.
    pub font_family: Option<SharedString>,
    /// Font size as a multiple of the editor font size. Overlapping scales multiply.
    pub font_scale: Option<f32>,
    /// Baseline shift in editor font sizes, positive is up. The first decoration covering a byte wins.
    pub baseline_shift: Option<f32>,
}

impl TextDecoration {
    /// Create a text decoration from a UTF-8 byte range and a GPUI style.
    pub fn new(range: Range<usize>, style: HighlightStyle) -> Self {
        Self {
            range,
            style,
            font_family: None,
            font_scale: None,
            baseline_shift: None,
        }
    }

    /// Render this range in another font family, e.g. a monospace one for inline code.
    pub fn with_font_family(mut self, font_family: impl Into<SharedString>) -> Self {
        self.font_family = Some(font_family.into());
        self
    }

    /// Render this range at `font_scale` times the editor font size, e.g. 0.75 for a subscript.
    pub fn with_font_scale(mut self, font_scale: f32) -> Self {
        self.font_scale = Some(font_scale);
        self
    }

    /// Shift this range's baseline by `baseline_shift` editor font sizes, positive is up.
    pub fn with_baseline_shift(mut self, baseline_shift: f32) -> Self {
        self.baseline_shift = Some(baseline_shift);
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct DecorationCollectionId(usize);

/// An independently managed collection of [`TextDecoration`]s.
///
/// This is the GPUI Component counterpart of Monaco's
/// [`IEditorDecorationsCollection`](https://microsoft.github.io/monaco-editor/typedoc/interfaces/editor_editor_api.editor.IEditorDecorationsCollection.html).
pub struct TextDecorationCollection<M: DecoratedMode = EditorMode> {
    state: WeakEntity<InputBaseState<M>>,
    id: DecorationCollectionId,
}

impl<M: DecoratedMode> Clone for TextDecorationCollection<M> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            id: self.id,
        }
    }
}

impl<M: DecoratedMode> std::fmt::Debug for TextDecorationCollection<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextDecorationCollection")
            .field("id", &self.id)
            .finish()
    }
}

impl<M: DecoratedMode> TextDecorationCollection<M> {
    /// Replace all decorations in this collection.
    ///
    /// This corresponds to Monaco's
    /// [`IEditorDecorationsCollection.set`](https://microsoft.github.io/monaco-editor/typedoc/interfaces/editor_editor_api.editor.IEditorDecorationsCollection.html#set).
    pub fn set(&self, decorations: Vec<TextDecoration>, cx: &mut App) {
        let _ = self.state.update(cx, |state, cx| {
            let decorations = normalize(&state.text, decorations);
            if M::decoration_collections(&mut state.extras)
                .0
                .set(self.id, decorations)
            {
                cx.notify();
            }
        });
    }

    /// Add decorations to this collection.
    ///
    /// This corresponds to Monaco's
    /// [`IEditorDecorationsCollection.append`](https://microsoft.github.io/monaco-editor/typedoc/interfaces/editor_editor_api.editor.IEditorDecorationsCollection.html#append).
    pub fn append(&self, decorations: Vec<TextDecoration>, cx: &mut App) {
        let _ = self.state.update(cx, |state, cx| {
            let decorations = normalize(&state.text, decorations);
            if M::decoration_collections(&mut state.extras)
                .0
                .append(self.id, decorations)
            {
                cx.notify();
            }
        });
    }

    /// Remove all decorations from this collection.
    ///
    /// This corresponds to Monaco's
    /// [`IEditorDecorationsCollection.clear`](https://microsoft.github.io/monaco-editor/typedoc/interfaces/editor_editor_api.editor.IEditorDecorationsCollection.html#clear).
    pub fn clear(&self, cx: &mut App) {
        self.set(Vec::new(), cx);
    }

    /// Return the UTF-8 byte ranges in this collection.
    ///
    /// This corresponds to Monaco's
    /// [`IEditorDecorationsCollection.getRanges`](https://microsoft.github.io/monaco-editor/typedoc/interfaces/editor_editor_api.editor.IEditorDecorationsCollection.html#getRanges).
    pub fn get_ranges(&self, cx: &App) -> Vec<Range<usize>> {
        self.state
            .read_with(cx, |state, _| {
                state
                    .decoration_collections()
                    .0
                    .get(self.id)
                    .unwrap_or_default()
                    .iter()
                    .map(|decoration| decoration.range.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// An independently managed collection of geometric range decorations.
///
/// Clones address the same collection. Dropping a handle does not clear it; use
/// [`Self::clear`] to empty it or [`Self::dispose`] to release it permanently.
/// Operations on a disposed collection or a dropped editor are harmless no-ops.
pub struct RangeDecorationCollection<M: DecoratedMode = EditorMode> {
    state: WeakEntity<InputBaseState<M>>,
    id: DecorationCollectionId,
}

impl<M: DecoratedMode> Clone for RangeDecorationCollection<M> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            id: self.id,
        }
    }
}

impl<M: DecoratedMode> std::fmt::Debug for RangeDecorationCollection<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RangeDecorationCollection")
            .field("id", &self.id)
            .finish()
    }
}

impl<M: DecoratedMode> RangeDecorationCollection<M> {
    /// Replace only this owner's decorations, clipping ranges to UTF-8 boundaries.
    pub fn set(&self, decorations: Vec<RangeDecoration>, cx: &mut App) {
        let _ = self.state.update(cx, |state, cx| {
            let decorations = normalize(&state.text, decorations);
            if M::decoration_collections(&mut state.extras)
                .1
                .set(self.id, decorations)
            {
                cx.notify();
            }
        });
    }

    /// Append decorations, preserving their paint order.
    pub fn append(&self, decorations: Vec<RangeDecoration>, cx: &mut App) {
        let _ = self.state.update(cx, |state, cx| {
            let decorations = normalize(&state.text, decorations);
            if M::decoration_collections(&mut state.extras)
                .1
                .append(self.id, decorations)
            {
                cx.notify();
            }
        });
    }

    /// Empty this collection without invalidating its handles.
    pub fn clear(&self, cx: &mut App) {
        self.set(Vec::new(), cx);
    }

    /// Release this collection, invalidating all of its cloned handles.
    pub fn dispose(&self, cx: &mut App) {
        let _ = self.state.update(cx, |state, cx| {
            if M::decoration_collections(&mut state.extras)
                .1
                .entries
                .remove(&self.id)
                .is_some()
            {
                cx.notify();
            }
        });
    }

    /// Read tracked UTF-8 byte ranges in insertion order.
    pub fn get_ranges(&self, cx: &App) -> Vec<Range<usize>> {
        self.state
            .read_with(cx, |state, _| {
                state
                    .decoration_collections()
                    .1
                    .get(self.id)
                    .unwrap_or_default()
                    .iter()
                    .map(|decoration| decoration.range.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Both text styles and geometric decorations share normalization and edit affinity.
#[doc(hidden)]
pub trait TrackedDecoration {
    fn range(&self) -> &Range<usize>;
    fn range_mut(&mut self) -> &mut Range<usize>;
    /// An empty range still means something, e.g. a block over one empty line.
    fn keeps_empty(&self) -> bool {
        false
    }
}

impl TrackedDecoration for TextDecoration {
    fn range(&self) -> &Range<usize> {
        &self.range
    }
    fn range_mut(&mut self) -> &mut Range<usize> {
        &mut self.range
    }
}

impl TrackedDecoration for RangeDecoration {
    fn range(&self) -> &Range<usize> {
        &self.range
    }
    fn range_mut(&mut self) -> &mut Range<usize> {
        &mut self.range
    }
    fn keeps_empty(&self) -> bool {
        matches!(
            self.style,
            RangeDecorationStyle::Block | RangeDecorationStyle::Bar
        )
    }
}

/// A balanced interval index over stable insertion-order entries. Each midpoint
/// stores the maximum end of its subtree, so one document-spanning decoration
/// does not force a scan of every preceding decoration on each frame.
struct DecorationIndex {
    indices: Vec<usize>,
    max_ends: Vec<usize>,
}

impl DecorationIndex {
    fn new<T: TrackedDecoration>(decorations: &[T]) -> Self {
        let mut indices: Vec<_> = (0..decorations.len()).collect();
        indices.sort_unstable_by_key(|&ix| (decorations[ix].range().start, ix));
        let mut index = Self {
            max_ends: vec![0; indices.len()],
            indices,
        };
        index.build(decorations, 0..decorations.len());
        index
    }

    fn build<T: TrackedDecoration>(&mut self, decorations: &[T], span: Range<usize>) -> usize {
        if span.is_empty() {
            return 0;
        }
        let mid = span.start + span.len() / 2;
        let end = decorations[self.indices[mid]]
            .range()
            .end
            .max(self.build(decorations, span.start..mid))
            .max(self.build(decorations, mid + 1..span.end));
        self.max_ends[mid] = end;
        end
    }

    // Returns the number of visited nodes, allowing deterministic complexity tests.
    fn query<T: TrackedDecoration>(
        &self,
        decorations: &[T],
        span: Range<usize>,
        range: &Range<usize>,
        matches: &mut Vec<usize>,
    ) -> usize {
        if span.is_empty() || range.is_empty() {
            return 0;
        }
        let mid = span.start + span.len() / 2;
        if self.max_ends[mid] < range.start {
            return 1;
        }
        let mut visited = 1 + self.query(decorations, span.start..mid, range, matches);
        let ix = self.indices[mid];
        let candidate = decorations[ix].range();
        if candidate.start < range.end {
            if candidate.end > range.start
                || (candidate.is_empty() && candidate.start == range.start)
            {
                matches.push(ix);
            }
            visited += self.query(decorations, mid + 1..span.end, range, matches);
        }
        visited
    }
}

struct DecorationEntries<T> {
    decorations: Vec<T>,
    index: DecorationIndex,
}

impl<T: TrackedDecoration> DecorationEntries<T> {
    fn new(decorations: Vec<T>) -> Self {
        let index = DecorationIndex::new(&decorations);
        Self { decorations, index }
    }

    fn reindex(&mut self) {
        self.index = DecorationIndex::new(&self.decorations);
    }
}

#[doc(hidden)]
pub struct DecorationCollections<T = TextDecoration> {
    entries: BTreeMap<DecorationCollectionId, DecorationEntries<T>>,
    next_id: usize,
}

impl<T> Default for DecorationCollections<T> {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
            next_id: 0,
        }
    }
}

impl<T: TrackedDecoration> DecorationCollections<T> {
    fn create(&mut self, decorations: Vec<T>) -> DecorationCollectionId {
        let id = DecorationCollectionId(self.next_id);
        self.next_id += 1;
        self.entries.insert(id, DecorationEntries::new(decorations));
        id
    }

    fn set(&mut self, id: DecorationCollectionId, decorations: Vec<T>) -> bool {
        let Some(current) = self.entries.get_mut(&id) else {
            return false;
        };
        *current = DecorationEntries::new(decorations);
        true
    }

    fn append(&mut self, id: DecorationCollectionId, decorations: Vec<T>) -> bool {
        let Some(current) = self.entries.get_mut(&id) else {
            return false;
        };
        current.decorations.extend(decorations);
        current.reindex();
        true
    }

    fn get(&self, id: DecorationCollectionId) -> Option<&[T]> {
        self.entries
            .get(&id)
            .map(|entry| entry.decorations.as_slice())
    }

    pub(super) fn adjust_for_edit(&mut self, edited_range: &Range<usize>, inserted_len: usize) {
        for entry in self.entries.values_mut() {
            let len = entry.decorations.len();
            if len == 0 || entry.index.max_ends[len / 2] <= edited_range.start {
                continue;
            }
            let mut remap = Vec::with_capacity(len);
            let mut retained = 0;
            entry.decorations.retain_mut(|decoration| {
                *decoration.range_mut() =
                    adjust_range_for_edit(decoration.range(), edited_range, inserted_len);
                let keep = !decoration.range().is_empty() || decoration.keeps_empty();
                remap.push(if keep { retained } else { usize::MAX });
                retained += usize::from(keep);
                keep
            });
            // Anchor transforms are monotone. Preserve start ordering and remap
            // removed entries rather than sorting on every keystroke: O(n).
            entry.index.indices.retain_mut(|ix| {
                *ix = remap[*ix];
                *ix != usize::MAX
            });
            entry.index.max_ends.resize(retained, 0);
            entry.index.build(&entry.decorations, 0..retained);
        }
    }

    pub(super) fn clear(&mut self) {
        for entry in self.entries.values_mut() {
            *entry = DecorationEntries::new(Vec::new());
        }
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = &[T]> {
        self.entries
            .values()
            .map(|entry| entry.decorations.as_slice())
    }

    /// Query visible buffer spans (not the intervening folded-away text).
    /// Rebuilds happen on mutations, never in layout/paint. Preserve owner/item
    /// order after deduplicating ranges crossing multiple visible lines.
    pub(super) fn intersecting(&self, ranges: &[Range<usize>]) -> Vec<&T> {
        let mut result = Vec::new();
        for entry in self.entries.values() {
            let mut matches = Vec::new();
            for range in ranges {
                entry.index.query(
                    &entry.decorations,
                    0..entry.decorations.len(),
                    range,
                    &mut matches,
                );
            }
            matches.sort_unstable();
            matches.dedup();
            result.extend(matches.into_iter().map(|ix| &entry.decorations[ix]));
        }
        result
    }
}

pub(crate) fn adjust_range_for_edit(
    range: &Range<usize>,
    edited_range: &Range<usize>,
    inserted_len: usize,
) -> Range<usize> {
    let removed_len = edited_range.end.saturating_sub(edited_range.start);
    let shift = |offset: usize| {
        if inserted_len >= removed_len {
            offset.saturating_add(inserted_len - removed_len)
        } else {
            offset.saturating_sub(removed_len - inserted_len)
        }
    };

    if edited_range.is_empty() {
        let start = if range.start < edited_range.start {
            range.start
        } else {
            shift(range.start)
        };
        let end = if range.end <= edited_range.start {
            range.end
        } else {
            shift(range.end)
        };
        return start..end;
    }

    let inserted_end = edited_range.start + inserted_len;
    let start = if range.start <= edited_range.start {
        range.start
    } else if range.start >= edited_range.end {
        shift(range.start)
    } else {
        edited_range.start
    };
    let end = if range.end <= edited_range.start {
        range.end
    } else if range.end >= edited_range.end {
        shift(range.end)
    } else {
        inserted_end
    };
    start..end
}

/// The font properties a text decoration changes, which also change glyph widths.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct FontOverride {
    pub(crate) family: Option<SharedString>,
    pub(crate) weight: Option<FontWeight>,
    pub(crate) style: Option<FontStyle>,
    pub(crate) scale: Option<f32>,
    pub(crate) raise: Option<f32>,
}

impl FontOverride {
    pub(crate) fn apply(&self, font: &mut Font) {
        if let Some(family) = &self.family {
            font.family = family.clone();
        }
        if let Some(weight) = self.weight {
            font.weight = weight;
        }
        if let Some(style) = self.style {
            font.style = style;
        }
    }
}

/// Non-overlapping spans sorted by start; the first item covering a byte wins.
fn first_wins_spans<V>(items: impl Iterator<Item = (Range<usize>, V)>) -> Vec<(Range<usize>, V)>
where
    V: Clone,
{
    let mut spans: Vec<(Range<usize>, V)> = Vec::new();
    for (range, value) in items {
        let mut uncovered = vec![range];
        for (covered, _) in &spans {
            uncovered = uncovered
                .into_iter()
                .flat_map(|range| {
                    [
                        range.start..range.end.min(covered.start),
                        range.start.max(covered.end)..range.end,
                    ]
                })
                .filter(|range| !range.is_empty())
                .collect();
        }
        spans.extend(uncovered.into_iter().map(|range| (range, value.clone())));
    }
    spans.sort_by_key(|(range, _)| range.start);
    spans
}

fn product_spans(items: Vec<(Range<usize>, f32)>) -> Vec<(Range<usize>, f32)> {
    let mut edges: Vec<usize> = items
        .iter()
        .flat_map(|(range, _)| [range.start, range.end])
        .collect();
    edges.sort_unstable();
    edges.dedup();
    let mut spans: Vec<(Range<usize>, f32)> = Vec::new();
    for segment in edges.windows(2) {
        let mut covering = items
            .iter()
            .filter(|(range, _)| range.start <= segment[0] && segment[1] <= range.end)
            .map(|(_, scale)| *scale)
            .peekable();
        if covering.peek().is_none() {
            continue;
        }
        let scale: f32 = covering.product();
        match spans.last_mut() {
            Some((range, last)) if range.end == segment[0] && *last == scale => {
                range.end = segment[1];
            }
            _ => spans.push((segment[0]..segment[1], scale)),
        }
    }
    spans
}

/// Font overrides per byte span; earlier layers and items win per property, except scale.
pub(crate) fn font_override_spans(
    layers: &[&[TextDecoration]],
) -> Vec<(Range<usize>, FontOverride)> {
    let decorations = || layers.iter().flat_map(|layer| layer.iter());
    let families = first_wins_spans(decorations().filter_map(|decoration| {
        Some((decoration.range.clone(), decoration.font_family.clone()?))
    }));
    let weights =
        first_wins_spans(decorations().filter_map(|decoration| {
            Some((decoration.range.clone(), decoration.style.font_weight?))
        }));
    let styles =
        first_wins_spans(decorations().filter_map(|decoration| {
            Some((decoration.range.clone(), decoration.style.font_style?))
        }));
    let scales = product_spans(
        decorations()
            .filter_map(|decoration| Some((decoration.range.clone(), decoration.font_scale?)))
            .collect(),
    );
    let raises = first_wins_spans(
        decorations()
            .filter_map(|decoration| Some((decoration.range.clone(), decoration.baseline_shift?))),
    );
    let mut edges: Vec<usize> = families
        .iter()
        .map(|(range, _)| range)
        .chain(weights.iter().map(|(range, _)| range))
        .chain(styles.iter().map(|(range, _)| range))
        .chain(scales.iter().map(|(range, _)| range))
        .chain(raises.iter().map(|(range, _)| range))
        .flat_map(|range| [range.start, range.end])
        .collect();
    edges.sort_unstable();
    edges.dedup();
    fn value_at<V: Clone>(spans: &[(Range<usize>, V)], offset: usize) -> Option<V> {
        let ix = spans.partition_point(|(range, _)| range.end <= offset);
        spans
            .get(ix)
            .filter(|(range, _)| range.start <= offset)
            .map(|(_, value)| value.clone())
    }
    let mut result: Vec<(Range<usize>, FontOverride)> = Vec::new();
    for segment in edges.windows(2) {
        let font_override = FontOverride {
            family: value_at(&families, segment[0]),
            weight: value_at(&weights, segment[0]),
            style: value_at(&styles, segment[0]),
            scale: value_at(&scales, segment[0]),
            raise: value_at(&raises, segment[0]),
        };
        if font_override == FontOverride::default() {
            continue;
        }
        match result.last_mut() {
            Some((range, last)) if range.end == segment[0] && *last == font_override => {
                range.end = segment[1];
            }
            _ => result.push((segment[0]..segment[1], font_override)),
        }
    }
    result
}

fn normalize<T: TrackedDecoration>(text: &Rope, decorations: Vec<T>) -> Vec<T> {
    decorations
        .into_iter()
        .filter_map(|mut decoration| {
            // Reject reversed ranges before clipping, which could otherwise turn a
            // reversed pair within a multibyte character into a nonempty range.
            let reversed = decoration.range().start > decoration.range().end;
            if reversed || (decoration.range().is_empty() && !decoration.keeps_empty()) {
                return None;
            }
            let range = text.clip_offset(decoration.range().start, Bias::Left)
                ..text.clip_offset(decoration.range().end, Bias::Right);
            if range.is_empty() && !decoration.keeps_empty() {
                return None;
            }
            *decoration.range_mut() = range;
            Some(decoration)
        })
        .collect()
}

impl<M: DecoratedMode> InputBaseState<M> {
    pub(crate) fn decoration_collections(
        &self,
    ) -> (
        &DecorationCollections,
        &DecorationCollections<RangeDecoration>,
    ) {
        M::decoration_collections_ref(&self.extras)
    }

    /// Create an independently owned collection of geometric range decorations.
    ///
    /// Ranges use UTF-8 byte offsets and the same tracking as text decorations:
    /// insertion at either edge does not expand the range, insertion inside does,
    /// replacement clips overlapping anchors, and deletion removes empty ranges.
    /// Undo, redo, whole-document replacement and formatting apply these same edit
    /// transforms; decorations themselves are not undo history, so deleted ranges
    /// are not resurrected by undo. Folding changes projection, not stored ranges.
    ///
    /// Fills paint behind frames; within each style, later collections/items paint
    /// over earlier ones. Neither affects text layout, hit testing or focus. The
    /// default color is the editor foreground (12% opacity for fills).
    /// Collections live until explicitly disposed or the editor is dropped.
    pub fn create_range_decorations_collection(
        &mut self,
        decorations: Vec<RangeDecoration>,
        cx: &mut Context<Self>,
    ) -> RangeDecorationCollection<M> {
        let decorations = normalize(&self.text, decorations);
        let id = M::decoration_collections(&mut self.extras)
            .1
            .create(decorations);
        cx.notify();
        RangeDecorationCollection {
            state: cx.entity().downgrade(),
            id,
        }
    }

    /// Create an independently managed collection of text decorations.
    ///
    /// This follows Monaco's
    /// [`createDecorationsCollection`](https://microsoft.github.io/monaco-editor/typedoc/interfaces/editor_editor_api.editor.ICodeEditor.html#createDecorationsCollection)
    /// ownership model. Ranges use UTF-8 byte offsets into [`Self::value`].
    ///
    /// Decoration ranges follow text edits and do not need to be set again
    /// after each change. Insertions at a range boundary do not expand the
    /// range, matching Monaco's
    /// [`NeverGrowsWhenTypingAtEdges`](https://microsoft.github.io/monaco-editor/typedoc/enums/editor_editor_api.editor.TrackedRangeStickiness.html#NeverGrowsWhenTypingAtEdges)
    /// behavior. Decorations are not rendered while the input is masked.
    /// Collections live until their [`InputBaseState`] is dropped.
    ///
    /// Collections are layered in insertion order; the first collection wins
    /// when overlapping decorations set the same [`HighlightStyle`] property
    /// or font family. Font family, weight and style also affect soft wrapping.
    /// Callers should avoid conflicting overlaps within one collection.
    pub fn create_decorations_collection(
        &mut self,
        decorations: Vec<TextDecoration>,
        cx: &mut Context<Self>,
    ) -> TextDecorationCollection<M> {
        let decorations = normalize(&self.text, decorations);
        let id = M::decoration_collections(&mut self.extras)
            .0
            .create(decorations);
        cx.notify();
        TextDecorationCollection {
            state: cx.entity().downgrade(),
            id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometric_collections_share_utf8_normalization_and_edit_affinity() {
        let mut collections = DecorationCollections::<RangeDecoration>::default();
        let text = Rope::from("héllo world");
        let first = collections.create(normalize(
            &text,
            vec![
                RangeDecoration::new(2..4),
                RangeDecoration::new(2..1),
                RangeDecoration::new(100..200),
            ],
        ));
        let second = collections.create(normalize(&text, vec![RangeDecoration::new(7..12)]));
        assert_eq!(collections.get(first).unwrap()[0].range(), &(1..4));
        assert_eq!(collections.get(first).unwrap().len(), 1);
        collections.adjust_for_edit(&(1..1), 2);
        assert_eq!(collections.get(first).unwrap()[0].range(), &(3..6));
        collections.adjust_for_edit(&(6..6), 1);
        assert_eq!(collections.get(first).unwrap()[0].range(), &(3..6));
        collections.adjust_for_edit(&(4..4), 2);
        assert_eq!(collections.get(first).unwrap()[0].range(), &(3..8));
        collections.adjust_for_edit(&(3..8), 0);
        assert!(collections.get(first).unwrap().is_empty());
        assert!(!collections.get(second).unwrap().is_empty());
        collections.entries.remove(&first);
        let third = collections.create(vec![]);
        assert_ne!(third, first);
        assert!(!collections.set(first, vec![RangeDecoration::new(0..1)]));
        assert!(collections.get(second).is_some());
    }

    #[test]
    fn visible_query_preserves_layers_and_skips_folded_spans() {
        let mut collections = DecorationCollections::<RangeDecoration>::default();
        let first = collections.create(vec![
            RangeDecoration::new(90..100),
            RangeDecoration::new(0..100),
            RangeDecoration::new(40..50), // hidden in a fold
            RangeDecoration::new(0..5),
        ]);
        collections.create(vec![RangeDecoration::new(2..4)]);
        let ranges = |collections: &DecorationCollections<RangeDecoration>| {
            collections
                .intersecting(&[0..5, 90..100])
                .iter()
                .map(|d| d.range().clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(ranges(&collections), vec![90..100, 0..100, 0..5, 2..4]);
        collections.adjust_for_edit(&(0..0), 1);
        assert_eq!(ranges(&collections), vec![91..101, 1..101, 1..6, 3..5]);
        collections.set(first, vec![RangeDecoration::new(50..60)]);
        assert_eq!(ranges(&collections), vec![3..5]);
    }

    #[test]
    fn interval_index_culls_large_collections_even_with_a_spanning_range() {
        let mut decorations: Vec<_> = (0..100_000)
            .map(|ix| RangeDecoration::new(ix * 10..ix * 10 + 5))
            .collect();
        decorations.push(RangeDecoration::new(0..1_000_000));
        let index = DecorationIndex::new(&decorations);
        for query in [
            0..1,
            500_000..500_020,
            999_990..1_000_001,
            1_000_000..1_000_010,
        ] {
            let mut matches = Vec::new();
            let visited = index.query(&decorations, 0..decorations.len(), &query, &mut matches);
            matches.sort_unstable();
            let expected: Vec<_> = decorations
                .iter()
                .enumerate()
                .filter_map(|(ix, d)| {
                    (d.range.start < query.end && d.range.end > query.start).then_some(ix)
                })
                .collect();
            assert_eq!(matches, expected);
            assert!(visited < 100, "visited {visited} nodes for {query:?}");
        }
    }

    #[test]
    fn interval_index_matches_linear_reference_for_overlaps_and_mutations() {
        let mut collections = DecorationCollections::<RangeDecoration>::default();
        let id = collections.create(
            (0..512)
                .map(|ix| {
                    let start = (ix * 37) % 997;
                    RangeDecoration::new(start..start + ix % 61 + 1)
                })
                .collect(),
        );
        for edit in [0..0, 300..450, 900..1100] {
            collections.adjust_for_edit(&edit, 3);
            for start in (0..1100).step_by(13) {
                let query = start..start + 17;
                let expected: Vec<_> = collections
                    .get(id)
                    .unwrap()
                    .iter()
                    .filter(|d| d.range.start < query.end && d.range.end > query.start)
                    .map(|d| d.range.clone())
                    .collect();
                let actual: Vec<_> = collections
                    .intersecting(&[query])
                    .iter()
                    .map(|d| d.range.clone())
                    .collect();
                assert_eq!(actual, expected);
            }
        }
    }

    #[test]
    fn collections_are_independent_and_ranges_are_clipped() {
        let text = Rope::from("héllo");
        let first_style = HighlightStyle {
            font_weight: Some(gpui::FontWeight::BOLD),
            ..Default::default()
        };
        let second_style = HighlightStyle {
            background_color: Some(gpui::red()),
            ..Default::default()
        };
        let mut collections = DecorationCollections::default();

        let first = collections.create(normalize(
            &text,
            vec![TextDecoration::new(2..4, first_style)],
        ));
        let second = collections.create(normalize(
            &text,
            vec![TextDecoration::new(5..100, second_style)],
        ));

        assert_ne!(first, second);
        assert_eq!(
            collections.get(first),
            Some(&[TextDecoration::new(1..4, first_style)][..])
        );
        assert_eq!(
            collections.get(second),
            Some(&[TextDecoration::new(5..6, second_style)][..])
        );

        assert!(collections.append(first, vec![TextDecoration::new(4..5, second_style)]));
        assert_eq!(
            collections.get(first),
            Some(
                &[
                    TextDecoration::new(1..4, first_style),
                    TextDecoration::new(4..5, second_style),
                ][..]
            )
        );

        assert!(collections.set(first, Vec::new()));
        assert_eq!(collections.get(first), Some(&[][..]));
        assert_eq!(
            collections.get(second),
            Some(&[TextDecoration::new(5..6, second_style)][..])
        );
    }

    #[test]
    fn font_override_spans_let_earlier_layers_and_items_win_per_property() {
        let style = HighlightStyle::default();
        let first = [
            TextDecoration::new(4..8, style).with_font_family("Mono"),
            TextDecoration::new(6..10, style).with_font_family("Serif"),
            TextDecoration::new(0..20, style),
        ];
        let second = [TextDecoration::new(2..12, style).with_font_family("Other")];
        assert_eq!(TextDecoration::new(0..1, style).font_family, None);
        let family = |name: &str| FontOverride {
            family: Some(name.into()),
            ..Default::default()
        };
        assert_eq!(
            font_override_spans(&[&first[..], &second[..]]),
            vec![
                (2..4, family("Other")),
                (4..8, family("Mono")),
                (8..10, family("Serif")),
                (10..12, family("Other")),
            ]
        );

        let bold = HighlightStyle {
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        };
        let italic = HighlightStyle {
            font_style: Some(FontStyle::Italic),
            font_weight: Some(FontWeight::LIGHT),
            ..Default::default()
        };
        let layer = [
            TextDecoration::new(0..6, bold).with_font_family("Mono"),
            TextDecoration::new(4..8, italic),
        ];
        assert_eq!(
            font_override_spans(&[&layer[..]]),
            vec![
                (
                    0..4,
                    FontOverride {
                        family: Some("Mono".into()),
                        weight: Some(FontWeight::BOLD),
                        style: None,
                        ..Default::default()
                    }
                ),
                (
                    4..6,
                    FontOverride {
                        family: Some("Mono".into()),
                        weight: Some(FontWeight::BOLD),
                        style: Some(FontStyle::Italic),
                        ..Default::default()
                    }
                ),
                (
                    6..8,
                    FontOverride {
                        family: None,
                        weight: Some(FontWeight::LIGHT),
                        style: Some(FontStyle::Italic),
                        ..Default::default()
                    }
                ),
            ]
        );
    }

    #[test]
    fn font_override_spans_multiply_scales_and_first_raise_wins() {
        let style = HighlightStyle::default();
        let layer = [
            TextDecoration::new(0..8, style).with_font_scale(2.),
            TextDecoration::new(4..12, style)
                .with_font_scale(0.5)
                .with_baseline_shift(0.35),
            TextDecoration::new(6..10, style).with_baseline_shift(-0.2),
        ];
        let scaled = |scale: f32, raise: Option<f32>| FontOverride {
            scale: Some(scale),
            raise,
            ..Default::default()
        };
        assert_eq!(
            font_override_spans(&[&layer[..]]),
            vec![
                (0..4, scaled(2., None)),
                (4..8, scaled(1., Some(0.35))),
                (8..12, scaled(0.5, Some(0.35))),
            ]
        );
    }

    #[test]
    fn decoration_ranges_follow_text_edits() {
        let style = HighlightStyle::default();
        let mut collections = DecorationCollections::default();
        let collection = collections.create(vec![TextDecoration::new(2..6, style)]);

        collections.adjust_for_edit(&(0..0), 2);
        assert_eq!(
            collections.get(collection),
            Some(&[TextDecoration::new(4..8, style)][..])
        );

        collections.adjust_for_edit(&(6..6), 2);
        assert_eq!(
            collections.get(collection),
            Some(&[TextDecoration::new(4..10, style)][..])
        );

        collections.adjust_for_edit(&(4..10), 3);
        assert_eq!(
            collections.get(collection),
            Some(&[TextDecoration::new(4..7, style)][..])
        );

        assert_eq!(adjust_range_for_edit(&(2..6), &(2..2), 2), 4..8);
        assert_eq!(adjust_range_for_edit(&(2..6), &(6..6), 2), 2..6);
        assert_eq!(adjust_range_for_edit(&(2..6), &(2..6), 3), 2..5);
    }
}
