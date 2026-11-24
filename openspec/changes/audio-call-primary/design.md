# Design

## Context

See proposal.md. `video_split_button` in `crates/app/src/views/call_controls.rs` is the chat header control. Its main click always calls `start_call(..., true)` and shows `IconName::Video`. The chevron menu lists video, then audio, except for group Meet now, where the main button is labelled Meet now and the menu lists only audio. Shortcuts `alt-shift-a` and `alt-shift-v` in `crates/app/src/views/shortcuts.rs` already start audio and video separately. The profile card in `crates/app/src/views/profile_card.rs` has two buttons.

## Goals / Non-Goals

**Goals:**

- One-to-one primary click starts audio and shows the phone.
- Video remains one menu item on that control.
- Group Meet now stays a video start.

**Non-Goals:**

- Reordering the profile card.
- Changing shortcut bindings.
- New icons or spacing. Phone and video icons already exist.

## Decisions

1. Branch the main button on `meet_now`. One-to-one: phone icon, tooltip audio call, `start_call(..., false)`. Group: keep the current video Meet now button.
   - Alternative: make every primary click audio, including Meet now. Rejected. Meet now in Teams is the video meeting.
2. One-to-one menu order becomes audio (same as the primary action) then video, so the menu does not lead with the action the button no longer performs. Group menu stays audio-only, as today.
3. No new strings if `calls.chrome.audio_call` and `calls.chrome.video_call` already exist. Reuse them.

## Risks / Trade-offs

- [Tests or selectors assume `HEADER_VIDEO_CALL_TAG` means a video start] → Update those assertions with the button. The tag can stay; the action changes only for one-to-one.

## Migration Plan

UI only. Rollback is restoring the video click on the one-to-one button.

## Open Questions

None.
