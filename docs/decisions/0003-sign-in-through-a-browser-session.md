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

On Linux and macOS the app starts Chrome or Chromium with remote debugging and its own profile, shows it for login, and hides it again afterwards. Without a Chromium browser it does the same with a Firefox browser over WebDriver BiDi. The app finds Chrome in the usual install locations. Only that managed Chrome may be stopped via the debug port. A second launch brings the existing instance forward. The UI can paint from the local cache before the network answers.

Tokens are not written to disk. The browser profile holds the session.

## Consequences

- Linux needs a Chrome or Chromium install, or a Firefox browser. macOS takes the first Chromium browser it finds: Chrome, Chromium, Edge, Brave, Helium or Vivaldi. Each runs with the app's own profile, so the user's everyday profile is never touched.
- Without a Chromium browser, Linux and macOS start a Firefox browser instead (Firefox, Zen, LibreWolf, Floorp) with its own `rusty-teams-firefox` profile. `RUSTY_TEAMS_SIGN_IN_BROWSER` names an executable to use either way.
- Firefox has no CDP any more, so it runs over WebDriver BiDi. Its transport answers the few CDP calls the session layer sends (evaluate, bindings, injected scripts, navigate, reload) with BiDi commands, so nothing above the transport knows the difference.
- Firefox allows one BiDi session per browser and keeps it after the socket closes. The app holds one shared connection; when a crashed earlier run left its session behind, the app restarts the managed Firefox.
- Firefox stops with SIGTERM: BiDi `browser.close` leaves the process and its profile lock alive on macOS. Its profile turns off Safe Mode, which hard stops would trigger and which hangs a headless start.
- Forks print their own version in `--version` (`Helium 0.21.3.1 (Chromium 157.0.8103.78)`), so the user agent takes the highest major in that line.
- Sign-in bugs are browser and profile bugs, not a second token store to rotate.
- Headless Chromium's user agent must look like desktop Chrome on that OS, or Microsoft rejects the session. Headless Firefox keeps its normal user agent.
