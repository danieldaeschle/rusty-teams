## 1. Server mute

- [x] 1.1 Record `server_muted` on the roster member and silence the local mic without posting
- [x] 1.2 Let the next unmute clear that mute until `serverMuted` drops
- [x] 1.3 Unit tests for the sync decision and the roster flag

## 2. Microphone selection

- [x] 2.1 Return the noise-suppression track only for Low
- [x] 2.2 Match named software-path inputs by listed name. Windows is exact and case-insensitive. Linux and macOS also match when either name contains the other
- [x] 2.3 Fall back to the default microphone while sharing computer sound, and stay silent only when that also fails
- [x] 2.4 Report a device switch as applied only when start succeeds on Windows. On Linux and macOS report it when the index is selected
- [x] 2.5 Unit tests for track choice, name match, and the start result

## 3. H.264 level

- [x] 3.1 Raise the answer level from `max-fs` and `max-mbps` without lowering it
- [x] 3.2 Keep the 1080p test and add the level-3.0 720p case

## 4. Screen share scale

- [x] 4.1 Scale live share frames with libyuv to 8160 macroblocks before I420 and reject short buffers
- [x] 4.2 Unit test the 2560 by 1440 budget and the short buffer

## 5. Device list poll

- [x] 5.1 Diff endpoint ids and feed the existing debounce on Linux and macOS once a second
- [x] 5.2 Unit test that a remove-and-return inside the quiet period is one event

## 6. Verify

- [x] 6.1 `openspec validate call-media-hardening`
- [x] 6.2 `cargo test -p calling --lib` and `cargo test -p app-call-model`
