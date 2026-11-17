---
status: accepted
date: 2026-11-15
---

# ADR 0020: macOS tray and close-to-tray match Linux

## Context

Linux already has a StatusNotifierItem tray and closes the main window into the tray because GPUI cannot hide a Wayland window (ADR 0012). macOS daily-driver use needs the same affordances: a menu-bar icon with presence / Open / Quit, and Settings → Close to tray. Native window hide is unreliable enough across GPUI versions that reusing the Linux “really close, reopen from tray” path is simpler than a separate Cocoa hide path.

Windows already uses `tray-icon`; macOS can share that crate instead of inventing a third tray stack.

## Decision

- On macOS, the tray is `tray-icon` (same implementation as Windows).
- Close to tray on macOS follows the Linux path: the main window closes for real; the tray (or a second start) opens a fresh one.
- Message toasts stay on the existing GPUI / Windows card path for now. macOS Notification Center is a follow-up once the shared desktop-notification policy from the Linux toast work lands.

## Consequences

- One close-to-tray mental model for Linux and macOS.
- Signing / notarization still matter for TCC and Gatekeeper; packaging scripts may ship a bare Mach-O or a signed `.app`.
- Contributors should not add a macOS-only hide path without revisiting this ADR.
