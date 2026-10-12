# gpui-component 0.7.1 patches

Upstream: crates.io `gpui-component 0.7.1`. Wired in via `[patch.crates-io]` in the root `Cargo.toml`.

| File | Change | Why |
|---|---|---|
| `src/styled.rs` | `popover_style`: no `.shadow(popover_shadow(..))`, radius `theme.radius_lg` | popups have no ring or shadow, radius 10 via `radius_lg` without touching buttons and inputs |
| `src/styled.rs` | `#[allow(dead_code)]` on `popover_shadow` | no caller left |
| `src/popover.rs` | `dropdown_popup`: no animated shadow, `cx` renamed `_cx`, drop `popover_shadow` import | same as `popover_style` |
| `src/menu/menu_item.rs` | hover and selected use `tokens.sidebar_accent` / `sidebar_accent_foreground` instead of `accent` | menu hover is `POPUP_HOVER`, `accent` stays for buttons and lists |
| `src/menu/popup_menu.rs` | separator `border_b(1px)` in `tokens.sidebar_accent` | 1px `POPUP_HOVER` line, full width |
| `src/tooltip.rs` | no `border_1`, `border_color`, `shadow_md` | tooltips match borderless popups |
| `src/button/button.rs` | pointer cursor on every interactive button, not only link/text | consistent with the app's own clickable elements |
