# gpui-base 0.7.1 patches

Upstream: crates.io `gpui-base 0.7.1` (git `87d10ae5`, `crates/base`). Wired in via `[patch.crates-io]` in the root `Cargo.toml`.

## Changed files

| File | Change | Why |
|---|---|---|
| `Cargo.toml` | empty `[workspace]` table at the end | `cargo test --manifest-path vendor/gpui-base/Cargo.toml` works inside the app workspace (and nested worktrees) |
| `src/input/editor/decorations.rs` | `TextDecoration.font_family`, `with_font_family()`, `FontOverride` (family, weight, style), `font_override_spans()` reading `font_family` + `style.font_weight` / `style.font_style`, `adjust_range_for_edit` made `pub(crate)`, doc on layering | font per byte range; first collection / item wins per property |
| `src/input/editor/display_map/text_wrapper.rs` | `font_overrides` on `TextWrapper`, `split_run_by_font_overrides()`, `set_font_overrides()` / `adjust_font_overrides()`, `measured_wrap_boundaries` measures by range, shared `changed_ranges()` / `rewrap_rows_of()` | soft wrap measures mono / bold / italic ranges in their own font |
| `src/input/editor/display_map/wrap_map.rs`, `display_map.rs`, `mod.rs` | pass-through for `set_font_overrides`, export `split_run_by_font_overrides` | plumbing |
| `src/input/base/element.rs` | prepaint pushes font-override spans into the display map; render runs split by font override | glyphs, caret, selection and hit testing use the same shaped runs |
| `src/input/base/state.rs` | `set_hanging_indents(markers)` on multi-line states (textarea and editor) | list lines: continuation rows start under the text after `•`, `◦`, `12.` |
| `src/input/editor/display_map/text_wrapper.rs` | `hanging_indents` on `TextWrapper`, `hanging_indent_at()`, `measured_wrap_boundaries(.., hanging_indent, ..)`, `shift_span()` shared with inline metrics; a line with a marker always wraps as `WrappingIndent::Same` | wrap continuation rows at `wrap_width - marker width`; the existing `LineItem.indent` / `LineLayout.wrap_indent` path then shifts rendering, caret, selection, hit testing and up/down moves |
| `src/input/editor/display_map/wrap_map.rs`, `display_map.rs` | pass-through for `set_hanging_indents`, adjust on edit | plumbing |
| `src/input/base/kind.rs` | `TextareaExtras` (text + range decorations) as `TextareaMode::Extras`; edit tracking and reset hooks; sealed `DecoratedMode` trait for both multi-line modes | decorations on `TextareaState` (composer stays a textarea) |
| `src/input/editor/decorations.rs` | `TextDecorationCollection<M = EditorMode>`, `RangeDecorationCollection<M = EditorMode>`, `create_*_collection` on `impl<M: DecoratedMode>`; `DecorationCollections` / `TrackedDecoration` `pub` + `#[doc(hidden)]` | same API on textarea and editor; default type param keeps editor code unchanged |
| `src/input/editor/mod.rs`, `src/input/mod.rs` | doc no longer says decorations are editor-only; export `DecoratedMode`, `TextareaExtras` | docs, API |
| `src/input/editor/display_map/text_wrapper.rs` | lines with inline tokens wrap through `measured_wrap_boundaries` too (`atomic` token ranges, token width + shaped text segments) instead of gpui's `LineWrapper` | mention lines honour font overrides and hanging indent |
| `src/input/base/state.rs`, `src/input/editor/display_map/*`, `src/input/base/element.rs` | `set_line_indents(Vec<(line start, Pixels)>)`: every row of a line starts at its indent and wraps that much earlier; `LineLayout.first_indent` feeds caret, selection, hit testing and up/down; combines with a hanging indent; without one, continuation rows start at the line indent, not under leading spaces | nested list levels, quote gap and code padding without padding characters in the text |
| `src/input/editor/decorations.rs` | `RangeDecorationStyle::{Pill, Block, Bar}`, `RangeDecoration::with_border()` / `with_radius()`; Block and Bar keep empty ranges (`TrackedDecoration::keeps_empty`) and the index finds them | rounded inline code pills, full-width code block fill, quote bar; a block over one empty line |
| `src/input/base/element.rs` | `layout_range_decoration_quads()` paints pills, blocks and bars as quads below the fills | rounded corners and borders, which paths cannot draw |
| `src/input/base/state.rs` | `apply_edits(&[(Range, String)])` on all states | several edits in one undo step without touching inline tokens between them |
| `src/input/base/inline_tokens.rs`, `src/input/base/token_presentation.rs` | `InlineToken::block()` / `is_block()` (part of Hash and Eq), `InlineTokenContext::is_block()`, token layout cache keeps a `Size` per token (`sizes`) | a token that sits alone on its own row at its rendered height, e.g. an image preview in the composer |
| `src/input/editor/display_map/text_wrapper.rs` | `InlineMetric { range, width, height }` replaces `(Range, Pixels)`; `measured_wrap_boundaries(.., forced, ..)` ends a row at each block token edge; `LineItem.blocks`, `LineSummary.block_rows` / `block_height`, `BlockExtent` dimension; `TextWrapper::row_top` / `row_height` / `row_at_y` / `content_height`; `LineLayout::row_top` / `row_height` / `row_at_y`; `InputLine.height` | rows are no longer uniform: a row that is exactly one block token has the token's height, lookups stay O(log n) |
| `src/input/editor/display_map/display_map.rs`, `wrap_map.rs`, `mod.rs` | `DisplayMap::row_top` / `row_height` / `row_at_y` / `content_height` / `content_rows`, `InlineMetric` plumbing and export | plumbing |
| `src/input/base/element.rs` | block tokens measured with `MinContent` height (always, also off-screen); caret top and height, selection corners, Block / Bar decoration height, visible range, scroll size and token placement use the row helpers | caret, selection and paint follow the taller row |
| `src/input/base/state.rs`, `movement.rs`, `mode.rs` | `scroll_to`, hit testing, IME bounds, `line_and_position_for_offset` and Up / Down use the row helpers; `update_auto_grow(display_map, line_height)` counts `content_rows` | click, scroll and arrow keys land on the right row; auto grow fits the image |
| `src/input/base/mode.rs`, `element.rs` | `update_auto_grow` lets the auto-grow height exceed `max_rows` by the rows block tokens add, up to `BLOCK_GROWTH_FACTOR` (2) times `max_rows`; the textarea min height uses `mode.rows()` | an image does not use up the text row budget |
| `src/input/base/element.rs` | `layout_cursors` also follows the caret on a frame where an auto-grow viewport changed height (`viewport_changed`), not only when the selection changed | text typed after a block token, or loaded content, stays visible while the viewport is still growing |

## Limits

- Syntax-highlight (tree-sitter / LSP) bold or italic still wraps with the base font; only decorations feed wrapping.
- Hanging-indent markers are dropped by an edit inside them; the app sets them again on change.
- The unwrapped longest-line width (soft wrap off) ignores font overrides.
- Block tokens are textarea-only and need soft wrap: no folds, line numbers, ghost text or touch handles. Without soft wrap a block token shares its row with text.

## Re-apply on a gpui-kit upgrade

1. Check the new `gpui-base` version pinned by `gpui-kit`.
2. `git diff <vendor commit> HEAD -- vendor/gpui-base > /tmp/gpui-base.patch` (vendor commit: `chore: vendor gpui-base 0.7.1 unchanged`).
3. Replace `vendor/gpui-base` with the new registry source (`~/.cargo/registry/src/*/gpui-base-<ver>/`, drop `.cargo-ok`), commit it alone.
4. `git apply -3 /tmp/gpui-base.patch`, fix conflicts.
5. `cargo test --manifest-path vendor/gpui-base/Cargo.toml --lib -- input` and `cargo build -p app`; `Cargo.lock` must show `gpui-base` without `source`.
6. Delete this folder and the `[patch]` entry once upstream ships the features.
