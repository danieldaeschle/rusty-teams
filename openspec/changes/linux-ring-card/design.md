# Design

## Context

See proposal.md for why. `NotificationCenter::sync_windows` in `crates/app/src/notify/center.rs` returns immediately on Linux when `desktop` is connected, after `sync_desktop`. That skips `open_call_window` for `CallToast::Ring`, so `RingView` never opens. The card already exists for the path where no notification service is connected: `popup_options` (`WindowKind::PopUp`, no focus steal) and `RingView` (decline, audio accept, video accept). Desktop actions already call `accept_ring` / `decline_ring`. The ring card is an existing popup surface in `docs/design/islands/SPEC.md`; this change does not add a surface or look tokens.

## Goals / Non-Goals

**Goals:**

- Open, place, and close ring cards while the desktop notification service stays connected.
- Keep publishing the ring notification and handling its actions.
- Leave chat toasts on the desktop-notification path. Meeting cards are the companion change in this pull request.

**Non-Goals:**

- Changing Hyprland or Quickshell config.
- Opening in-app windows for chat toasts.
- A new window kind or new chrome.

## Decisions

1. After `sync_desktop`, sync ring cards and return. Do not fall through into the full window sync, because that also opens chat toast windows.
   - Alternative: delete the early return. Rejected. Chat toasts would become in-app windows as well, which the current Linux spec does not want.
2. Reuse `place_call_toast` / `close_call_window`, filtered to `CallToast::Ring` and `CallToast::Meeting`. Chat toast keys stay out of that filter. The meeting card itself is specified in `linux-meeting-card`.
3. Keep `popup_options` as they are. The card the user asked for is that popup.
4. Card actions stay on `RingView`. The notification closes because the ring leaves `ringing()` and the next `sync_desktop` drops it. If a card action does not reach that sync in the same turn, queue the same desktop close the notification actions already queue before answering.

## Risks / Trade-offs

- [Hyprland does not map `WindowKind::PopUp` or does not deliver clicks] → The card is created and still invisible or dead. Mitigation: verify on this Hyprland session (Quickshell still owning notifications). If the popup does not show or does not take the accept click, switch that ring window to a normal floating window in the same change. Do not add a second card.
- [Two alerts for one ring] → Accepted. The notification stays so the call is visible when the card is on another monitor or behind the main window.
- [Stale ring card after accept] → The filtered close path must run whenever ring ids disappear, including while `desktop` is connected.

## Migration Plan

App change only. Rollback is restoring the early return after `sync_desktop`. No data migration.

## Open Questions

None.
