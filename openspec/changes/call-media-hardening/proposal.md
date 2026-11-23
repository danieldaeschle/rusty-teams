## Why

Live calls still lose the local microphone after an organizer mute, send the wrong noise-suppression track when a named microphone cannot be opened, stall 720p when Teams answers H.264 level 3.0, and spend the screen-share budget converting a full Retina or 5K frame. Linux and macOS also ignore device arrival until the user refreshes the device list.

## What Changes

- A server mute of the local participant closes the microphone in the call and in the button. The next toggle clears that mute. A later organizer mute applies again.
- Low is the only mode that publishes the WebRTC noise-suppression track. A named software-path microphone matches the device list name exactly, or the capture fails with the existing notice. Selecting a device counts as applied only when recording or playout actually starts.
- An H.264 answer level rises to the lowest table level that covers the peer's `max-fs` and `max-mbps`. The level is never lowered. An answer without those limits is unchanged.
- Screen share scales the captured frame down to the 1080p macroblock budget before the I420 conversion. A short buffer produces no frame.
- On Linux and macOS the call compares the audio device list once a second and feeds the existing device-change path when an endpoint appears or disappears.

## Capabilities

### New Capabilities

- `call-server-mute`: Organizer mute of the local participant is reflected on the microphone control and can be cleared by the local user.

### Modified Capabilities

- `linux-call-audio`: Named software-path capture fails clearly, and Low is the only device-track mode that enables classical noise suppression.
- `macos-call-audio`: Same named-microphone and noise-suppression rules on the CoreAudio path.
- `camera-video-quality`: Answered H.264 level follows the peer's explicit frame-size limits, including level 3.0 answers that still allow 720p.
- `screen-share`: Shared frames are reduced to the send budget before colorspace conversion.

## Impact

Calling, the call model, and the screen-capture path. No new encoder, decoder, or GPU scaler. Device hotplug on Linux and macOS reuses the existing follow-up that Windows already runs from endpoint notifications.
