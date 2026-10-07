# gpui-base 0.7.1 patches

Upstream: crates.io `gpui-base 0.7.1` (git `87d10ae5`, `crates/base`). Wired in via `[patch.crates-io]` in the root `Cargo.toml`.

## Changed files

| File | Change | Why |
|---|---|---|
| `Cargo.toml` | empty `[workspace]` table at the end | `cargo test --manifest-path vendor/gpui-base/Cargo.toml` works inside the app workspace (and nested worktrees) |
| `src/input/editor/decorations.rs` | `TextDecoration.font_family`, `with_font_family()`, `font_family_spans()`, `adjust_range_for_edit` made `pub(crate)`, doc on layering | font family per byte range; first collection / item wins |
| `src/input/editor/display_map/text_wrapper.rs` | `font_families` on `TextWrapper`, `split_run_by_font_families()`, `set_font_families()` / `adjust_font_families()`, `measured_wrap_boundaries` measures by range, shared `changed_ranges()` / `rewrap_rows_of()` | soft wrap measures font-family ranges in their own font |
| `src/input/editor/display_map/wrap_map.rs`, `display_map.rs`, `mod.rs` | pass-through for `set_font_families`, export `split_run_by_font_families` | plumbing |
| `src/input/base/element.rs` | prepaint pushes font-family spans into the display map; render runs split by font family | glyphs, caret, selection and hit testing use the same shaped runs |
| `src/input/base/state.rs` | `set_hanging_indents(markers)` on multi-line states (textarea and editor) | list lines: continuation rows start under the text after `•`, `◦`, `12.` |
| `src/input/editor/display_map/text_wrapper.rs` | `hanging_indents` on `TextWrapper`, `hanging_indent_at()`, `measured_wrap_boundaries(.., hanging_indent, ..)`, `shift_span()` shared with inline metrics; a line with a marker always wraps as `WrappingIndent::Same` | wrap continuation rows at `wrap_width - marker width`; the existing `LineItem.indent` / `LineLayout.wrap_indent` path then shifts rendering, caret, selection, hit testing and up/down moves |
| `src/input/editor/display_map/wrap_map.rs`, `display_map.rs` | pass-through for `set_hanging_indents`, adjust on edit | plumbing |
| `src/input/base/kind.rs` | `TextareaExtras` (text + range decorations) as `TextareaMode::Extras`; edit tracking and reset hooks; sealed `DecoratedMode` trait for both multi-line modes | decorations on `TextareaState` (composer stays a textarea) |
| `src/input/editor/decorations.rs` | `TextDecorationCollection<M = EditorMode>`, `RangeDecorationCollection<M = EditorMode>`, `create_*_collection` on `impl<M: DecoratedMode>`; `DecorationCollections` / `TrackedDecoration` `pub` + `#[doc(hidden)]` | same API on textarea and editor; default type param keeps editor code unchanged |
| `src/input/editor/mod.rs`, `src/input/mod.rs` | doc no longer says decorations are editor-only; export `DecoratedMode`, `TextareaExtras` | docs, API |

## Limits

- Only the font family changes wrapping. Bold/italic `HighlightStyle` decorations still wrap with the regular-weight font.
- Lines containing inline tokens wrap through gpui's `LineWrapper` with the base font only and ignore their hanging indent.
- Hanging-indent markers are dropped by an edit inside them; the app sets them again on change.
- The unwrapped longest-line width (soft wrap off) ignores font families.

## Re-apply on a gpui-kit upgrade

1. Check the new `gpui-base` version pinned by `gpui-kit`.
2. `git diff <vendor commit> HEAD -- vendor/gpui-base > /tmp/gpui-base.patch` (vendor commit: `chore: vendor gpui-base 0.7.1 unchanged`).
3. Replace `vendor/gpui-base` with the new registry source (`~/.cargo/registry/src/*/gpui-base-<ver>/`, drop `.cargo-ok`), commit it alone.
4. `git apply -3 /tmp/gpui-base.patch`, fix conflicts.
5. `cargo test --manifest-path vendor/gpui-base/Cargo.toml --lib -- input` and `cargo build -p app`; `Cargo.lock` must show `gpui-base` without `source`.
6. Delete this folder and the `[patch]` entry once upstream ships the features.
