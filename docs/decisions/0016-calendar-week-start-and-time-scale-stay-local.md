---
status: proposed
date: 2026-11-15
---

# ADR 0016: Calendar work hours go to the mailbox, week start and time scale stay local

## Context

`GUARDRAILS.md` says settings Teams knows are saved where Teams saves them. The calendar view settings come from two places in Teams:

- Work days and work hours are the Outlook mailbox working hours. Graph exposes them as `/me/mailboxSettings`. The Teams client token is granted `MailboxSettings.ReadWrite` but not `MailboxSettings.Read`. A live read returned HTTP 200 on 2026-11-15.
- First day of week and the time scale are Outlook web (OWA) user options. Graph has no endpoint for them. The new Teams calendar reads them through the OWA options service.

The options were:

- write everything locally;
- reach the OWA options service from the fork;
- split by what Graph can reach.

## Decision

- Work days and hours are read from and written to the mailbox working hours through Graph. A local copy is only a cache for the grid.
- First day of week defaults to the regional format, which is where Teams takes it from when no OWA option is set. A local override can change it.
- The time scale is a local setting. Its default, 30 minutes, matches Teams.
- Default view and the last used span are fork settings and stay local.

## Consequences

- Changing work hours in the app changes them in Outlook and Teams too, and the other way round after the next read.
- A first-day or time-scale change in Outlook web does not reach the app, and an app change does not reach Outlook. This deviation is named in the change's commit.
- If the fork adopts the OWA options service later, first day and time scale move there, and this record is superseded.
- OpenSpec `calendar-view-settings` owns the observable behaviour.
