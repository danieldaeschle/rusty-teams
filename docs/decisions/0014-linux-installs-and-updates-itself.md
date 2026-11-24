---
status: accepted
date: 2026-11-14
---

# ADR 0014: The Linux binary installs itself

Revised 2026-11-25: a newer release updates the copy that owns its install, and opens the release page everywhere else.

## Context

The Linux release is a bare binary in a tarball. Users had to copy a `.desktop` file and an icon by hand, and only Windows updated itself from GitHub releases. The options were: an AppImage (FUSE mount on every start, its own update tooling), distro packages (one per distro, repositories to run), or letting the binary install itself into the user's data folder and reuse the Windows updater.

What decides an update is how the app was installed, not the operating system. A package manager already owns deb, rpm, Flatpak, and Snap. Swapping a Mach-O inside a signed macOS `.app` breaks the signature. An AppImage has its own update tool. A tarball copied into `~/.local` and the Windows per-user install can replace their own binary.

## Decision

On start the binary offers to install itself into `~/.local/share/rusty-teams/teams` and writes `~/.local/share/applications/rusty-teams.desktop` (`StartupWMClass=rusty-teams`, matching the window's app id). Build output from a cargo `target/` folder is copied, anything else is moved. "Don't ask again" can be undone in Settings > General, which also has Install, Replace and Remove. That install is not an update.

A release-tagged build looks for a newer GitHub tag when Settings > General > Updates is on (`updates.enabled`, one key on every OS, on by default). The old Linux key is not read.

- Windows downloads quietly and restarts when the app is idle and not in a call. The Windows deploy still swaps the binary from `update\`.
- Linux installed under `~/.local` asks first, downloads the platform asset, checks size, header, and the SHA-256 digest GitHub reports, then restarts only when the user asks.
- macOS, Linux deb/rpm/AppImage, and a binary that is not the `~/.local` install open the release page. Nothing is downloaded.
- Flatpak (`FLATPAK_ID`) does not check and hides the settings row.
- A build without a release tag does not check.

Not now holds for this run. Check now runs the same path on demand.

## Consequences

- One binary, no root, no FUSE; menu entry and dock matching work after one click.
- Package-managed installs stay on the package manager. The dialog does not replace them.
- The Windows ARM64 asset name stays `-windows-arm64.exe`.
