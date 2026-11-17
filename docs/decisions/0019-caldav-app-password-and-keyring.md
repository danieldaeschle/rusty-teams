---
status: accepted
date: 2026-11-16
---

# ADR 0019: CalDAV with app password and OS keyring

## Context

The next optional calendar after Google is CalDAV (Nextcloud, Fastmail, mailbox.org, Posteo, iCloud, Synology, Radicale). Per-provider OAuth would repeat Google Cloud work for every host. App passwords are how Thunderbird and DAVx⁵ already connect. Google OAuth tokens already sit in SQLite meta; an app password is long-lived and often unlocks mail as well as calendar.

Options were: OAuth per host, app password in SQLite like Google tokens, or app password in the OS keyring with metadata in SQLite.

## Decision

Connect CalDAV with server URL, username, and app password (Basic auth). Credentials stay on the device. The app password lives in the OS keyring (Secret Service, Keychain, Credential Manager). SQLite keeps URL, username, calendar list, and selection only. Diagnostics and support bundles redact URL userinfo, username, and password.

RSVP updates the local `PARTSTAT` when the user is an attendee. The UI treats the response as delivered to organizers only when the server advertises scheduling (RFC 6638 schedule-outbox). One write target sends invitations: Outlook when it is among the targets, otherwise CalDAV alone.

Digest auth (Baïkal default) is unsupported; the connect error says so. HTTPS must not downgrade to HTTP on redirect. TLS uses the system trust store; there is no «ignore certificate» switch.

## Consequences

- One client covers self-hosted and hosted CalDAV without a project OAuth app per provider.
- Flatpak needs `org.freedesktop.secrets`; packaging must request it.
- Users must create an app password (or enable Basic) on the server.
- Proton Calendar and Tuta remain outside this path (no CalDAV); read-only ICS is a later change.
