---
status: proposed
date: 2026-11-17
---

# ADR 0021: Linux releases also ship an AppImage, a deb and an rpm

## Context

The Linux release is a bare binary that installs and updates itself (ADR 0014). Users who manage software through their distribution, or who want a single file with a menu entry and delta updates, have no package. ADR 0014 left system packages open, as long as they do not offer the self-install. The Craft apps (PhotoCraft) already ship a Linux packaging set: one script staging an FHS tree, nfpm for deb and rpm, an AppImage with a self-integrating `AppRun`, all built on the oldest supported Ubuntu LTS. The Linux binary needs the glibc it was built against (the v0.4.0 release binary needs 2.43, newer than Ubuntu 24.04 or Debian 13 have), which is what packages built on an old distro, or a Flatpak with its own runtime, avoid.

## Decision

`packaging/linux/package.sh` builds `rusty-teams-<version>-linux-<arch>.{AppImage,AppImage.zsync,deb,rpm,tar.gz}` the same way PhotoCraft does, and `build.yml` attaches the AppImage, its `.zsync`, the deb and the rpm to every `v*` release, next to the unchanged bare binary and tarball. The Linux release job builds on Ubuntu 22.04, so the binary needs glibc 2.35. The desktop id stays `rusty-teams` (the window's app id); the AppStream component id is `io.github.danieldaeschle.RustyTeams`. A binary under `/usr` or `/opt`, or started from an AppImage (`APPIMAGE` set), counts as packaged and does not offer to install itself; outside the install folder it never updates itself either, so packages update through the package manager and the AppImage through its `.zsync`.

## Consequences

- deb and rpm list the libraries the binary loads with `dlopen` (Wayland, X11, Vulkan or EGL) as dependencies and the call stack's (PulseAudio, PipeWire, VA-API) as recommendations; they conflict with Microsoft's old `teams` package, which also owned `/usr/bin/teams`.
- The AppImage's `AppRun` writes `~/.local/share/applications/rusty-teams.desktop` like the self-install does; an entry the self-install wrote keeps its `Exec`.
- The Flatpak (`packaging/linux/flatpak-bundle.sh`, app id `io.github.danieldaeschle.RustyTeams`) builds but is broken and not released: the app starts the user's own browser with a debug port, and inside the sandbox it can neither see nor start the host's browsers. It needs the browser start to go through `flatpak-spawn --host`. A working Flatpak would end the glibc floor entirely, since it brings its own runtime.
- No package repository: deb and rpm are release downloads.
