---
status: accepted
date: 2026-11-12
---

# ADR 0003: Sign-in through a browser session

## Context

Rusty Teams must sign in to a company tenant without an app registration and without storing tokens on disk. Windows already does that with embedded WebView2. WebView2 does not exist on Linux or macOS.

Options were: a stored refresh token, an embedded engine per OS, or a real Chrome/Chromium window that the app starts and that holds the session.

## Decision

Windows keeps WebView2.

On Linux and macOS the app starts Chrome or Chromium with remote debugging and its own profile, shows it for login, and hides it again afterwards. The app finds Chrome in the usual install locations. Only that managed Chrome may be stopped via the debug port. A second launch brings the existing instance forward. The UI can paint from the local cache before the network answers.

Tokens are not written to disk. The browser profile holds the session.

## Consequences

- Linux and macOS need a Chrome or Chromium install.
- Sign-in bugs are browser and profile bugs, not a second token store to rotate.
- The headless user agent must look like desktop Chrome on that OS, or Microsoft rejects the session.
