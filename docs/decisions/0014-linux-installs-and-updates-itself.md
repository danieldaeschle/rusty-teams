---
status: proposed
date: 2026-11-14
---

# ADR 0014: The Linux binary installs and updates itself

## Context

The Linux release is a bare binary in a tarball. Users had to copy a `.desktop` file and an icon by hand, and only Windows updated itself from GitHub releases. The options were: an AppImage (FUSE mount on every start, its own update tooling), distro packages (one per distro, repositories to run), or letting the binary install itself into the user's data folder and reuse the Windows updater.

## Decision

On start the binary offers to install itself into `~/.local/share/rusty-teams/teams` and writes `~/.local/share/applications/rusty-teams.desktop` (`StartupWMClass=rusty-teams`, matching the window's app id). Build output from a cargo `target/` folder is copied, anything else is moved. "Don't ask again" can be undone in Settings > General, which also has Install, Replace and Remove. The installed copy updates itself from the same GitHub releases as Windows (`release_feed.rs`, `updater.rs`), but always with consent (`linux_update.rs`): when Settings > General > Automatic updates is on (the default), a newer release is offered in a dialog; Update downloads the `rusty-teams-<tag>-linux-amd64` asset with progress and checks size, ELF header and the SHA-256 digest GitHub reports; Restart now swaps the file and relaunches. Not now holds for the running session; the next start asks again. There is no idle restart and no install on quit on Linux: a download left for later is offered again on the next start (Restart now, no second download). Settings > General > Check now runs the same flow on demand. A build compares releases against its release tag, or against the workspace version (kept equal to the latest release) when built locally. Binaries outside the install folder never replace themselves; Settings > General still shows the switch and Check now and explains that the app has to be installed first.

## Consequences

- One binary, no root, no FUSE; menu entry and dock matching work after one click.
- Releases must keep publishing the bare `-linux-amd64` binary next to the tarball.
- Windows keeps its quiet download and idle restart; the two platforms differ on purpose.
- A self-built binary that is installed is offered releases newer than its workspace version; starting a newer build offers to replace the installed one.
- System packages (deb, rpm, Flatpak) stay possible and should simply not offer the install.
