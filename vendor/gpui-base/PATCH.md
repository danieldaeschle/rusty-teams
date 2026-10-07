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

## Limits

- Only the font family changes wrapping. Bold/italic `HighlightStyle` decorations still wrap with the regular-weight font.
- Lines containing inline tokens wrap through gpui's `LineWrapper` with the base font only.
- The unwrapped longest-line width (soft wrap off) ignores font families.
- Decorations exist on `EditorState` only, not `TextareaState`.

## Re-apply on a gpui-kit upgrade

1. Check the new `gpui-base` version pinned by `gpui-kit`.
2. `git diff <vendor commit> HEAD -- vendor/gpui-base > /tmp/gpui-base.patch` (vendor commit: `chore: vendor gpui-base 0.7.1 unchanged`).
3. Replace `vendor/gpui-base` with the new registry source (`~/.cargo/registry/src/*/gpui-base-<ver>/`, drop `.cargo-ok`), commit it alone.
4. `git apply -3 /tmp/gpui-base.patch`, fix conflicts.
5. `cargo test --manifest-path vendor/gpui-base/Cargo.toml --lib -- input` and `cargo build -p app`; `Cargo.lock` must show `gpui-base` without `source`.
6. Delete this folder and the `[patch]` entry once upstream ships the features.
