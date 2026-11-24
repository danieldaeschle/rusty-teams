# Proposal

## Why

On Hyprland a meeting reminder is a normal window, so the compositor tiles it. The incoming-call card is already a small floating window and stays in the corner.

## What Changes

- A meeting reminder on Linux opens the same floating card as an incoming call, including when a desktop notification service is connected.
- The desktop notification for that meeting stays.
- Chat toasts stay desktop notifications and do not open a card.
- Out of scope: Hyprland window rules, and changing the meeting card layout.

## Capabilities

### New Capabilities

- None.

### Modified Capabilities

- `linux-desktop-notifications`: a meeting reminder also shows a floating card on Linux and does not rely on a tiled popup window.

## Impact

- Linux notification windows for meeting reminders.
- No new surfaces, colours, or spacing.
