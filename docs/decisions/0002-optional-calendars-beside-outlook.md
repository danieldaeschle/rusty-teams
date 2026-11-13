---
status: accepted
date: 2026-11-11
---

# ADR 0002: Optional calendars beside Outlook

## Context

The calendar was Outlook only. People who also live in Google Calendar, or who have no usable mailbox, had no grid. Replacing Outlook would fight the Teams account. A second, separate calendar screen would hide conflicts.

Options were: Outlook only, Google instead of Outlook, or both in one grid with an explicit home calendar.

## Decision

Outlook stays the Teams calendar. Google is an optional extra account, stored on the device until the user disconnects. More providers can follow the same shape.

Events from the selected calendars share the week and month grid. The same meeting (same iCal UID or the same Teams join URL) is shown once, with its sources marked.

The user picks one home calendar. New meetings always write there, and may also write to extra targets. RSVP and reminder writes follow the calendar that owns the event. Google is polled. There is no Google push webhook.

If Outlook is missing or failing, a connected Google calendar still fills the grid, and a new meeting can still carry a Teams join link.

## Consequences

- The app is usable for calendar without a working Outlook mailbox.
- Credentials for non-Teams calendars stay local. They are not written into the Teams account.
- Each new provider needs its own connect, RSVP, and reminder path. The grid and the home-calendar rule stay.
