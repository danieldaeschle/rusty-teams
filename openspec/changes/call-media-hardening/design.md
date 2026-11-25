## Context

Auto and Low already send the platform ADM track. Music, Off, and computer sound use the software chain. Screen share converts native BGRA to I420 at capture size, and the encoder scales afterwards. `with_full_hd_level` only rewrites the level when both limits are at the 1080p thresholds.

## Goals / Non-Goals

**Goals:**

- Close the organizer-mute desync without posting a second mute.
- Stop Off and Music from restoring the noise-suppression track.
- Resolve a named microphone by the listed name. Windows compares that name exactly, ignoring ASCII case. Linux and macOS also match when either name contains the other, because cpal names differ from Pulse and CoreAudio.
- Treat Init/Start failure as a failed device switch on Windows. On Linux and macOS the switch counts once the index is selected.
- Raise an H.264 level only when the peer's own limits need it.
- Shrink screen frames with libyuv to at most 8160 macroblocks before `argb_to_i420`.
- Drive the existing device-change debounce from a once-per-second list diff on Linux and macOS.

**Non-Goals:**

- VAAPI decode, Media Foundation decode, and GPU scaling.
- A CoreAudio computer-sound tap.
- Excluding local playout from the Pulse monitor.
- Camera effects or the 720p camera ceiling during share.

## Decisions

1. Server mute stays on `Member.server_muted`. The call loop silences the local tracks and sends the muted update. It does not call `updateEndpointState`. An unmute while `server_muted` is still set marks the mute cleared until the roster drops `serverMuted`, so the next roster tick does not mute again.
2. `device_track` returns `track_with_ns` only for Low without music mode.
3. CPAL lookup uses the device-list name when the stored choice is an endpoint id. Windows compares that name with case-insensitive equality. Linux and macOS also accept a case-insensitive substring in either direction. An empty target never matches. While sharing computer sound, a failed named microphone falls back to the default microphone and shows that notice. Silence is used only when the default also fails.
4. On Windows, `select_input_index` and `select_output_index` return `selected && started`, including the Bluetooth retry. On Linux and macOS they return whether the index was selected, so one failed start does not keep retrying the same switch.
5. H.264 levels come from Table A-1. The rewrite replaces the last two hex digits when the needed level is higher. 1080p limits still land on level 4.0.
6. Screen scale uses libyuv `ARGBScale` with box filtering. The limit is the same 8160 macroblocks the screen receive request uses. Short buffers return no frame.
7. Linux and macOS poll `list_devices` once a second while the ADM is in use. `list_devices` acquires and releases a reference, so an already held ADM stays up. Changed endpoint ids go through `Debounce` and `follow_device_change`.

## Risks / Trade-offs

- A roster that keeps `serverMuted` after a successful unmute stays cleared until that flag drops. If the server rejects the unmute, the mic stays open locally until the next distinct server mute.
- Polling the device list once a second can miss a headset that drops and returns inside that second. The debounce still collapses a burst into one follow-up. That poll is the device-arrival path on Linux and macOS.

## Migration Plan

No stored-data migration. Rollback is reverting the change.

## Open Questions

None. Live echo, headset, and Retina share checks stay manual after the unit tests.
