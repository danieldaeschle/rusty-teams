## Context

Windows already downloads a release asset and restarts when idle and not in a call. Linux under `~/.local` already asks before the same swap. Other installs have no safe in-place replace.

## Goals / Non-Goals

**Goals:**

- Keep both existing self-update paths.
- Open the tag page when self-update is not allowed.
- One `updates.enabled` switch, hidden on Flatpak and on a build without a release tag.

**Non-Goals:**

- Signing release assets.
- Keeping the previous binary until the new one has started.

## Decisions

1. **Install kind, not OS, picks the path.** Windows always self-updates. Linux self-updates only when `self_update_allowed()` is true. Everything else that may check opens the release page.
2. **The notice does not need an asset.** Tag comparison is enough to open the page. The self-update path still requires the platform asset, including `-windows-arm64.exe`.
3. **The old Linux key is dropped.** `updates.enabled` is the only key. An old off value does not carry over.

## Risks / Trade-offs

- [A package install is copied into `~/.local` by hand] → That copy self-updates, which is the installed-copy rule.
