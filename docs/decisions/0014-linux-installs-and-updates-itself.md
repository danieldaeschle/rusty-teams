---
status: accepted
date: 2026-11-14
---

# ADR 0014: The Linux binary installs itself

Revised 2026-11-24: a newer release is a notice that opens the release page. The app does not replace its binary. The install half below is unchanged.

## Context

The Linux release is a bare binary in a tarball. Users had to copy a `.desktop` file and an icon by hand, and only Windows updated itself from GitHub releases. The options were: an AppImage (FUSE mount on every start, its own update tooling), distro packages (one per distro, repositories to run), or letting the binary install itself into the user's data folder and reuse the Windows updater.

## Decision

On start the binary offers to install itself into `~/.local/share/rusty-teams/teams` and writes `~/.local/share/applications/rusty-teams.desktop` (`StartupWMClass=rusty-teams`, matching the window's app id). Build output from a cargo `target/` folder is copied, anything else is moved. "Don't ask again" can be undone in Settings > General, which also has Install, Replace and Remove. That install is not an update. No platform downloads a release or replaces the running binary. A release-tagged build checks GitHub for a newer tag (`release_feed.rs`). Settings > General > Release notices is on by default (`updates.enabled`, the same key on every OS). A newer release opens a dialog; Open release opens that tag's page in the browser (`linux_update.rs`). Not now holds for this run. Check now runs the same check on demand. A build without a release tag does not check. When `FLATPAK_ID` is set there is no check and no release-notice row.

## Consequences

- One binary, no root, no FUSE; menu entry and dock matching work after one click.
- A newer release is a page in the browser. The app does not need a platform asset to notice it, and it does not swap its own binary.
- System packages (deb, rpm, Flatpak) stay possible and should simply not offer the install. Flatpak (`FLATPAK_ID`) also hides the release notice.
