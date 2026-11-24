## ADDED Requirements

### Requirement: Self-update follows the install

A release-tagged build with updates enabled SHALL replace itself only on Windows and on a Linux copy installed under the app data directory. Windows SHALL download quietly and restart only when idle and not in a call. The Linux copy SHALL ask before downloading and SHALL restart only when the user asks. The Windows ARM64 build SHALL select the `-windows-arm64.exe` asset.

#### Scenario: Windows stays on the quiet path

- **WHEN** a Windows release build finds a newer release and the app is idle and not in a call
- **THEN** the downloaded binary replaces the running one

#### Scenario: An installed Linux copy asks first

- **WHEN** a Linux copy under the app data directory finds a newer release
- **THEN** a dialog offers to download it, and the binary is unchanged until the user restarts

### Requirement: Other installs open the release page

macOS, a Linux package, an AppImage, and a binary that is not the installed Linux copy SHALL open the release page for a newer tag and SHALL NOT download an asset. Flatpak SHALL NOT check and SHALL NOT show the updates row. A build without a release tag SHALL NOT check.

#### Scenario: A notice opens the tag page

- **WHEN** an install that must not replace itself finds a newer tag
- **THEN** the dialog offers that tag's release page and the running binary stays in place

#### Scenario: Flatpak stays quiet

- **WHEN** `FLATPAK_ID` is set
- **THEN** there is no release check and no updates row in settings
