# gpui-pre 0.3.8 patches

Upstream: crates.io `gpui-pre 0.3.8`. Wired in via `[patch.crates-io]` in the root `Cargo.toml`. `examples/`, `tests/` and `docs/` are left out; their `[[example]]` and `[[test]]` targets are removed from `Cargo.toml`.

## Changed files

| File | Change | Why |
|---|---|---|
| `src/text_system/line.rs` | `paint_line` and `paint_line_background` place each wrapped line from its leftmost glyph (`segment_left_edges`) instead of its first glyph | Right-to-left runs come back from shaping in visual order over the unwrapped line, so a wrapped Arabic line started off its own right edge and overflowed (GitHub issue 64) |
