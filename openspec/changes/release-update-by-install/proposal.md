## Why

A newer release should replace the binary only when this install owns that file. Package-managed copies and a signed macOS app must not swap themselves. The release dialog stays the fallback for those installs.

## What Changes

- Windows keeps the quiet download, the idle restart, and the call guard.
- Linux installed under `~/.local` keeps the consent download and swap.
- macOS, deb, rpm, AppImage, and a binary outside `~/.local` open the release page and download nothing.
- Flatpak does not check and hides the settings row.
- One settings key, `updates.enabled`, default on, on every OS except Flatpak.

## Capabilities

### New Capabilities

- `release-update`: Choose self-update or the release page from how the app was installed.

### Modified Capabilities

## Impact

`release_feed`, `updater`, the Linux update dialog, the Windows idle poll, and Settings > General. The Windows ARM64 asset suffix stays.
