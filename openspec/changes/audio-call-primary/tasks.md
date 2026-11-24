# Tasks

## 1. Primary call is audio

- [x] 1.1 In `crates/app/src/views/call_controls.rs`, the one-to-one primary control shows the phone and calls `start_call` with audio. Video stays in the menu. Group Meet now still starts video. Verify with a unit test of that branch if the action is extracted, otherwise with the existing header-control test updated for the new action, and with `cargo test -p app --lib`.
- [x] 1.2 Reuse `calls.chrome.audio_call` and `calls.chrome.video_call`. Do not add strings. Verify no new keys under the locale catalogs and `cargo test -p app --lib` passes.
