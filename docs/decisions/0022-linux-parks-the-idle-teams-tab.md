---
status: proposed
date: 2026-11-17
---

# ADR 0022: Linux parks the idle Teams tab in a Chromium browser

## Context

On Windows the WebView2 host parks the Teams page on `robots.txt` of its own origin after a quiet minute. The MSAL cache stays reachable for token refreshes and the running Teams web app is unloaded. On Linux the managed browser kept the whole Teams web app loaded, close to a gigabyte, although Rust sends most requests itself. Options were: leave it loaded, unload it some other way (memory pressure flags, discarding the tab), or park it the way Windows does.

## Decision

On Linux with a Chromium browser, the app parks the Teams tab the way Windows does: a minute after the session last opened or woke the tab, when no call holds it and the tab has its MSAL cache, it navigates the tab to the park URL over CDP. The session wakes it as before when it needs the running app.

## Consequences

- The managed Chromium drops from about 1 GB to a few hundred MB once parked.
- The quiet minute counts from the last open or wake, not from the last command. The session keeps its own connection to the tab, so the transport does not see commands. A page-run call that overlaps the navigation loses its page connection: a read wakes the tab and runs again, a write fails with `WriteInterrupted`. Most requests go out from Rust and do not run in the page.
- Firefox browsers (BiDi) and macOS are left out: the MSAL check runs over CDP, and nobody has run parking on macOS yet.
