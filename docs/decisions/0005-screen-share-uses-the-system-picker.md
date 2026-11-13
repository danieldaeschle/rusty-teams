---
status: accepted
date: 2026-11-12
---

# ADR 0005: Screen share uses the system picker where the OS forbids enumeration

## Context

On Windows and plain X11 the app can list windows and screens and show thumbnails. Under Wayland an app may not enumerate foreign windows. macOS wants the system capture consent dialog. The share button then stays empty even when PipeWire or ScreenCaptureKit is available.

Options were: keep the thumbnail list everywhere, or ask the desktop to pick the source.

## Decision

Under Wayland (`WAYLAND_DISPLAY` set, even if `DISPLAY` is also set) and on macOS, the share menu offers two actions and no thumbnails: entire screen, or a window. Each action opens the system dialog (ScreenCast portal, or ScreenCaptureKit). The desktop draws that dialog.

After the user confirms, frames use the existing share pipeline (banner, stop, stage). Cancelling leaves the call up and shows no error. If the portal or the macOS capture path is missing, the app says so.

Sharing the entire screen moves this app's own window aside so it is not in the capture. Windows and plain X11 keep the thumbnail list.

## Consequences

- KDE, GNOME, and Hyprland can differ in the dialog. The app's request stays the same.
- There is no thumbnail strip to maintain on Wayland or macOS.
- Capture of the call window itself stays as in ADR 0001.
