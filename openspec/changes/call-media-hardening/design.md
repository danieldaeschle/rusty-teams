## Context

Auto and Low already send the platform ADM track. Music, Off, and computer sound use the software chain. Screen share converts native BGRA to I420 at capture size, and the encoder scales afterwards. `with_full_hd_level` only rewrites the level when both limits are at the 1080p thresholds.

## Goals / Non-Goals

**Goals:**

- Close the organizer-mute desync without posting a second mute.
- Stop Off and Music from restoring the noise-suppression track.
- Resolve a named microphone by the name on the device list, case-insensitive, with no substring match.
- Treat Init/Start failure as a failed device switch on every OS.
- Raise an H.264 level only when the peer's own limits need it.
- Shrink screen frames to at most 8160 macroblocks before `argb_to_i420`.
- Drive the existing device-change debounce from a once-per-second list diff on Linux and macOS.

**Non-Goals:**

- VAAPI decode, Media Foundation decode, and GPU scaling.
- A CoreAudio computer-sound tap.
- Excluding local playout from the Pulse monitor.
- Camera effects or the 720p camera ceiling during share.

## Decisions

1. Server mute stays on `Member.server_muted`. The call loop silences the local tracks and sends the muted update. It does not call `updateEndpointState`. An unmute while `server_muted` is still set marks the mute cleared until the roster drops `serverMuted`, so the next roster tick does not mute again.
2. `device_track` returns `track_with_ns` only for Low without music mode.
3. CPAL lookup uses the device-list name when the stored choice is an endpoint id. Comparison is case-insensitive equality. Windows `open_device` returns an error when a named endpoint is missing or inactive.
4. `select_input_index` and `select_output_index` return `selected && started` on every OS. The Bluetooth retry stays Windows-only.
5. H.264 levels come from Table A-1. The rewrite replaces the last two hex digits when the needed level is higher. 1080p limits still land on level 4.0.
6. Screen scale halves with a 2x2 average while the half still covers the target, then one bilinear step. The limit is the same 8160 macroblocks the screen receive request uses. Short buffers return no frame.
7. Linux and macOS poll `list_devices` once a second while the ADM is in use. `list_devices` acquires and releases a reference, so an already held ADM stays up. Changed endpoint ids go through `Debounce` and `follow_device_change`.

## Risks / Trade-offs

- A roster that keeps `serverMuted` after a successful unmute stays cleared until that flag drops. If the server rejects the unmute, the mic stays open locally until the next distinct server mute.
- Polling the device list once a second can miss a headset that drops and returns inside that second. The debounce still collapses a burst into one follow-up.
- The Rust scaler is not libyuv `ARGBScale`. It is covered by a macroblock test and does not add an FFI binding.

## Migration Plan

No stored-data migration. Rollback is reverting the change.

## Open Questions

None. Live echo, headset, and Retina share checks stay manual after the unit tests.
