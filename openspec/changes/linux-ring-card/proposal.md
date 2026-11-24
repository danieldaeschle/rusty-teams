# Proposal

## Why

On Linux an incoming call only appears as a desktop notification whenever a notification service is connected. On this Hyprland session that service is Quickshell, so the in-app ring card never opens and answering depends on notification actions. The card is the surface that already has Decline, audio accept, and video accept.

## What Changes

- While a desktop notification service is connected, an incoming ring still opens the existing in-app ring card.
- The desktop notification for that ring stays (Decline, audio accept, and video accept when the ring is video).
- Chat toasts stay desktop notifications only. They do not gain an in-app window. Meeting reminder cards are the companion change in this pull request.
- Windows and macOS toast behaviour stays as it is.
- Out of scope: camera device duplicates, the default outgoing call being video, and treating a click on the notification body as accept.

## Capabilities

### New Capabilities

### Modified Capabilities

- `linux-desktop-notifications`: An incoming ring shows the in-app ring card in addition to the desktop notification when a notification service is connected.

## Impact

- Linux notification placement in the app notify center. The ring card is the existing popup surface; no new look tokens.
- Desktop notification publish and action handling stay. Accept and decline on either surface end the same ring.
- No change to Hyprland or Quickshell configuration. Those cannot turn the card back on while the app skips it.
