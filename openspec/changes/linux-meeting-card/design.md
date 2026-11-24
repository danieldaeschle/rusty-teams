## Context

Wayland exposes `WindowKind::PopUp` as a normal toplevel. Hyprland tiles it. The incoming-call card already switches to `WindowKind::Floating` on Linux. While a notification service is connected, only ring cards are opened; meeting reminders are left to that service, whose window Hyprland tiles.

## Goals / Non-Goals

**Goals:**

- Open the existing meeting card as a floating window on Linux whenever a reminder is due.
- Keep publishing the meeting desktop notification.
- Leave chat toasts as notifications only.

**Non-Goals:**

- A Hyprland window rule.
- A new meeting card layout.

## Decisions

1. The filter that keeps cards while the notification service is connected includes meeting reminders as well as rings.
2. Meeting cards use the same window options as the ring card, so Linux selects the floating kind and other systems stay on the existing popup options.

## Risks / Trade-offs

- The user may see the floating card and the desktop notification together, as with an incoming call.

## Migration Plan

Rebuild and restart the local binary. No data migration.

## Open Questions

None.
