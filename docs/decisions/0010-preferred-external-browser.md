---
status: accepted
date: 2026-11-13
---

# ADR 0010: Preferred browser for links and Google OAuth

## Context

Google Calendar Connect opens an authorize URL in a real browser. In-app http(s) links do the same. The managed Chrome profile used for Microsoft sign-in (ADR 0003) must stay untouched — mixing OAuth and Teams login in that profile would fight cookies and make Connect look like another Teams sign-in.

Options were: always the OS default handler, always the managed Teams Chrome, or a user-chosen browser that is separate from the Teams session.

## Decision

Settings stores a preferred external browser (or system default). Google authorize URLs and other in-app http(s) opens use that preference, then fall back to the OS default if the program is missing. The managed Teams Chrome launch path does not read this setting.

## Consequences

- Users who live in Firefox or Brave can complete Google Connect there without changing how Teams signs in.
- Discoverable browsers are listed once at startup; an unknown saved program falls back to the system handler.
- This is not a second Teams session browser. ADR 0003 still owns Microsoft login.
