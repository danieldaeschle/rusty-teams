# Proposal

## Why

On this machine the Logitech C930e appears twice under the same name. Linux exposes `/dev/video0` (the picture) and `/dev/video1` (metadata) with one label. The camera list shows both, and the metadata node cannot start. That is the camera that "does not turn on".

## What Changes

- A camera list on Linux offers only nodes that can capture video. A metadata node with the same name is omitted.
- The default camera is a capture node, not the metadata twin.
- One physical camera still appears once, under its existing name.
- If the capture node itself fails to open, the app still shows that error. This change does not invent a second recovery path.
- Out of scope: the ring card, and which call button is the default.

## Capabilities

### New Capabilities

### Modified Capabilities

- `camera-capture`: The camera list includes capture nodes only, so a metadata twin of the same camera is not offered and cannot be selected.

## Impact

- Linux camera enumeration used by the call and the prejoin camera menu.
- Windows and macOS lists stay as they are.
- No new settings surface.
