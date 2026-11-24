# Tasks

## 1. Ring card beside the desktop notification

- [x] 1.1 In `crates/app/src/notify/center.rs`, after `sync_desktop` while a notification service is connected, open, place, and close only `CallToast::Ring` windows via the existing call-window helpers. Do not open chat toast windows or meeting windows on that path. Verify with a unit test that the desktop-connected window set is rings only, and with `cargo test -p app --lib`.
- [x] 1.2 Keep publishing the ring desktop notification and its actions. Accept and decline on the ring card must drop that notification in the same turn the ring ends (queue the existing desktop close if the next sync would not). Verify the unit test covers the close-with-ring case and `cargo test -p app --lib` passes.

## 2. Hyprland check

- [x] 2.1 With Quickshell still owning notifications, confirm an incoming ring shows the ring card and the desktop notification together, and that Decline and audio accept on the card work. If the popup is invisible or does not take the click, use one normal floating window for that card only and re-check. Verify by exercising a ring on this Hyprland session.
