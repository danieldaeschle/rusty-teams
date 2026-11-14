---
status: accepted
date: 2026-11-13
---

# ADR 0009: Bundled Google OAuth Desktop client

## Context

Google Calendar Connect needs an OAuth Desktop client id. Asking every user to create one in Google Cloud Console, or to export shell variables before starting the Mac `.app`, fails for Dock and Start Menu launches. A settings field for the client id would still dump Cloud Console work on end users.

Options were: env vars only, a settings field for the client id, or one project client baked into release builds.

## Decision

Official release builds compile in the project Desktop client id (and optional secret) via `option_env!`. Runtime env vars still override for developers. Users only click Connect Google; tokens stay on the device (see ADR 0002). Release CI must supply the client id at compile time so Dock and Start Menu launches are not Outlook-only by accident.

## Consequences

- Connect works from a Dock-started macOS `.app` and from Windows/Linux installers without a terminal.
- The project owns the Google Cloud consent screen and verification path for the sensitive `calendar` scope.
- Local `cargo run` without the secret stays Outlook-only unless the developer exports the env vars.
- The client id is visible in the binary; the secret must never appear in UI, logs, or the store.
