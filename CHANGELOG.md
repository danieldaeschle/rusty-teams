# Changelog

## [0.3.0-joe.4] - 2026-11-12

### Fixed

- Camera capture prefers MJPEG 1080p/720p at 30 fps on USB 2.0 webcams (e.g. C930e)
  instead of falling back to YUYV 1080p at 5 fps; opens under 30 fps are rejected
  when a faster format exists.
- MJPEG frames decode locally (zune-jpeg → I420) with reusable buffers.
- UVC best-effort: disable exposure dynamic framerate and set 50 Hz power-line
  frequency; call reports log the chosen format plus capture fps / decode time.

## [0.3.0-joe.3] - 2026-11-12

### Fixed

- 1:1 escalation answers no longer emit an extra folded gallery m-line (`mid 3-0`),
  which triggered Teams `InvalidNumberOfMedia` / «Incorrect number of media».
- Same-peer renegotiation keeps a short grace after the answer timeout so a late
  camera answer (`recvonly`) can still be applied instead of rolling back send.
- Camera open survives UVC `CameraFormat` property failures (Closest NV12 first;
  AbsoluteHighest only as last resort).

## [0.3.0-joe.2] - 2026-11-12

### Added

- Camera video quality setting: Auto, 720p, or 1080p (no 4K), in call and prejoin
  camera menus; choice persists and applies to capture plus stage receive limits.

### Changed

- Auto / 1080p prefer 1080p capture and advertise matching send capabilities;
  stage receive uses 1080p-class budgets when allowed.
- CI tests run on Forgejo only; GitHub Actions are mirror stubs.

## [0.3.0-joe.1] - 2026-11-12

### Added

- Forgejo as source of truth (`devops-geek/rusty-teams`) with push mirror to GitHub.
- Linux release packages: `.deb`, `.rpm`, Arch `.pkg.tar.zst`, Flatpak bundle.
- Forgejo Actions builds for Windows amd64, Windows ARM64, and macOS universal.
- Local share preview window in the meeting chat (and in the mini call window).

### Fixed

- Wayland screen share via xdg-desktop-portal (KDE, GNOME, Hyprland) without the
  «shared source is gone» toast after confirming the desktop dialog.

## [0.3.0] - 2026-11-10

Upstream-aligned baseline on the fork before Forgejo SoT cutover.
