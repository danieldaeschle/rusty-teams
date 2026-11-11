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
| `src/tooltip.rs` | `PopupTriggerTooltip` (`popup_trigger_tooltip`, `popup_trigger_tooltip_text`): no tooltip while the element's popup is open | a tooltip never covers the menu its trigger opened, and does not come back while it is open |
| `src/button/button.rs` | no managed tooltip while the button is held `open` by its popover or dropdown menu | same, for Button triggers |
| `src/tooltip.rs` | managed tooltip requests carry a liveness token from the trigger's element state (`while_alive`); `TOOLTIP_DEBUG_SELECTOR` on `Tooltip` | a tooltip closes when its trigger is no longer rendered (e.g. "Jump to newest" after the button vanished); tests find tooltips |
| `src/message_scroller.rs` | `wheel_mask(bool)` builder, default on; off skips the `ScrollableMask` child | the mask scrolls the list in the capture phase, so an editor inside a row (channel thread reply) never got the wheel |
| `src/message_scroller.rs` | `MessageScrollerState::item_bounds(index)` | the typing bubble sits right under the last row and sticks above the composer once that row scrolls away |
