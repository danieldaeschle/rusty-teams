# Tasks

## 1. Capture nodes only

- [x] 1.1 Filter `list_cameras` on Linux so a node is listed only when it can capture video. Unknown capability stays listed. Verify with a unit test that a metadata node is dropped and a capture node with the same name is kept, and with `cargo test -p calling --lib camera`.
- [x] 1.2 Default selection uses the first remaining capture node. Verify the unit test for `resolve_camera` with `SystemDefault` after the filter, and `cargo test -p calling --lib camera`.

## 2. This machine

- [x] 2.1 On this C930e, the camera menu shows one Logitech Webcam C930e and starting the default camera opens `/dev/video0`. Verify by listing cameras from the app menu or `list_cameras` and starting the default camera once.
