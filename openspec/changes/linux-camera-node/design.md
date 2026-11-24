# Design

## Context

See proposal.md. `list_cameras` in `crates/calling/src/camera.rs` maps every device from `nokhwa` `query(ApiBackend::Auto)` into a menu entry. On this machine `v4l2-ctl --list-devices` shows one C930e with `/dev/video0` and `/dev/video1`, and both sysfs names are `Logitech Webcam C930e`. `video1` is the metadata node. `resolve_camera` on non-Windows uses `CameraIndex::Index(0)` for the default and the stored key for a named device. `camera_entries` does not drop duplicates.

## Goals / Non-Goals

**Goals:**

- Drop Linux nodes that cannot capture video before they become menu entries.
- Keep the capture node's name and make default selection open that node.

**Non-Goals:**

- A new camera setting.
- Changing format choice, MJPEG decode, or Windows device keys.

## Decisions

1. Filter by video-capture capability, not by "same name". Two different cameras can share a label. A metadata node is identified by lacking video capture (V4L2 metadata / no capture flag), which on this C930e is `/dev/video1`.
   - Alternative: hide the second device that has the same name. Rejected. It would drop a real second camera that the driver names the same, and it would keep a lone metadata node if it were listed first.
2. Do the filter in `list_cameras` on Linux only, so prejoin and the in-call menu both see one list.
3. Default stays `DeviceChoice::SystemDefault` and still resolves to the first remaining capture node.

## Risks / Trade-offs

- [A capture node is misclassified as metadata] → The camera disappears. Mitigation: treat unknown capability as capture, and only drop a node when the non-capture flag is positive.
- [nokhwa's index does not match `/dev/videoN`] → The filter must use the index nokhwa returns, not a guessed path. Verify the C930e list is one entry and that starting the default camera opens.

## Migration Plan

No stored key migration. A previously saved key for the metadata index simply no longer matches; the menu falls back to the remaining capture device. Rollback is restoring the unfiltered list.

## Open Questions

None.
