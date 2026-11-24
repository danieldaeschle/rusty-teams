# Proposal

## Why

The one-click call control in a chat starts a video call. In Teams the primary call is the normal audio call. Video is the extra choice.

## What Changes

- In a one-to-one chat, the primary call control starts an audio call and shows the phone, not the camera.
- Video call stays available from the same control's menu.
- A group "Meet now" stays a video meeting. That control is not the one-to-one call.
- Keyboard shortcuts for audio and video stay as they are.
- The profile card keeps separate audio and video buttons. Their order does not change in this change.
- Out of scope: the ring card, and the duplicate camera node.

## Capabilities

### New Capabilities

- `outgoing-call`: The one-click call from a chat is an audio call. Video is an explicit second choice.

### Modified Capabilities

## Impact

- Chat header call control. No new surface and no new look tokens. The phone and video icons already exist.
- Group Meet now and the profile card are unchanged.
