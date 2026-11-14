---
status: accepted
date: 2026-11-14
---

# ADR 0012: On Linux, close to tray closes the main window

## Context

"Close to tray" hides the main window on Windows (`ShowWindow`). GPUI has no call to hide a window on Wayland, and an `xdg_toplevel` cannot be unmapped without patching the GPUI Linux backend. The options were: minimize instead (the window stays in the task manager), force XWayland so X11 unmapping works (blurry with fractional scaling, loses Wayland screen share), patch GPUI to destroy and recreate the toplevel role, or close the window for real and open a new one from the tray.

## Decision

On Linux, with a tray host present, closing the main window closes it for real while the app keeps running (`QuitMode::Explicit`, `main_window.rs`). The tray icon, a second start or an incoming call open a new main window. The entities that must outlive the window (notifications with the tray, call windows, activity) are created with the first window and handed to every later one (`KeptParts` in `views/shell.rs`). Without a tray host, closing quits as before.

## Consequences

- Works the same on Wayland and X11, and the window really leaves the task manager.
- A reopened window starts from the cached state, not from the exact previous UI: open dialogs and scroll positions are gone, the selected chat and drafts stay.
- Anything new that has to survive a closed main window must live in `KeptParts` or outside the shell, not in a view created by `AppShell::new`.
- We do not patch GPUI.
