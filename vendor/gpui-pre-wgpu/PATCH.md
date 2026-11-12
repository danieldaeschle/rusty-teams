# gpui-pre-wgpu 0.3.8 patches

Upstream: crates.io `gpui-pre-wgpu 0.3.8`. Wired in via `[patch.crates-io]` in the root `Cargo.toml`.

## Changed files

| File | Change | Why |
|---|---|---|
| `src/cosmic_text_system.rs` | `add_fonts` removes installed faces whose family a bundled face also has | Fedora ships Noto Color Emoji as COLRv1 only, which swash cannot draw; the bundled CBDT face of the same family must win the fallback (GitHub issue 8) |
