---
status: proposed
date: 2026-11-14
---

# ADR 0013: The Microsoft Teams icon is downloaded on Linux, not shipped

## Context

The "Microsoft Teams" app icon option embeds Microsoft's logo in the binary. For the Linux menu entry and tray we want the real Teams icon, but shipping Microsoft's trademark in our release assets is a risk. The options were: ship the PNG, use an icon theme name (depends on the user's theme), draw a look-alike (worse than either), or let the user's machine fetch the icon from Microsoft.

## Decision

Linux builds do not embed `microsoft-teams-256.png`. On start the app downloads the favicon the Teams web client uses (`teams.public.onecdn.static.microsoft/.../microsoft_teams_logo_refresh_v2025.ico`, with the `<link id="teams-favicon">` of `teams.cloud.microsoft` as fallback) into `~/.local/share/icons/hicolor/<size>/apps/rusty-teams.png`. Once present it is the default app icon unless one was chosen in Appearance. Until then, and offline, the Rusty Teams logo is used. Windows and macOS keep the embedded icon.

## Consequences

- No Microsoft logo in Linux release assets; the icon comes from Microsoft to the user directly.
- The first start shows the Rusty Teams logo for a moment; the title bar picks up the Teams icon on the next start.
- If Microsoft moves the file and the page stops naming it, the Rusty Teams logo stays.
