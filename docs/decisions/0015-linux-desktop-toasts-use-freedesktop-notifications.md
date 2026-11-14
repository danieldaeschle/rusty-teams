---
status: proposed
date: 2026-11-14
---

# ADR 0015: Linux desktop toasts use freedesktop Notifications

## Context

Message, ring, and meeting toasts on Windows are GPUI popup windows placed with Win32 (topmost, no activate, work-area corner). On Linux those platform hooks are stubs, so the decision layer and sounds already run but nothing appears. Copying the Win32 card with GPUI fails on Wayland: the toolkit cannot hide or decorate a borderless always-on-top inactive popup the way Teams expects. Options were: keep Linux silent, force a broken GPUI popup, or use the desktop notification service.

## Decision

On Linux, toasts go through `org.freedesktop.Notifications`. Show/hide rules, texts, and actions stay the same as the Windows card. The app keeps playing its own notification and ring tones and asks the shell to stay silent. Leaving a notification in the desktop history after the popup times out, honouring desktop Do not disturb via `Inhibited` when present, and attaching a sender avatar when available are intentional extras beyond the Windows card. macOS stays stubbed until a separate change. Windows keeps the GPUI card.

## Consequences

- Linux users get corner notifications on KDE and GNOME without a second UI toolkit path.
- Behaviour can differ by shell (history, inline reply, Do not disturb property).
- Without a notification daemon the app logs once and still plays sounds.
- A future macOS path should follow the same “OS surface, shared policy” split.
