---
status: proposed
date: 2026-11-24
---

# ADR 0023: People's calendars come from free/busy, the list stays local

## Context

Teams shows calendars of other people in the organization under "People's calendars". The fork showed only the own Outlook calendar.

Checked live on 2026-11-24 with the Teams client token (it carries `Calendars.Read.Shared`):

- `POST /me/calendar/getSchedule` answers for every colleague: busy blocks always, subject and location where the owner shares details.
- `GET /users/{mail}/calendar/calendarView` answers only where the owner shared the calendar, otherwise 404.
- The mailbox calendar group "People's calendars" (`/me/calendarGroups`) was empty although Teams showed people there, and Graph cannot add a person's calendar to it.

The options were: `calendarView` per person, `getSchedule` for everyone, or both.

## Decision

- People's calendars load through `getSchedule`, one request per 20 people. A block shows the subject when the owner shares it, otherwise its status (Busy, Tentative, Away, Working elsewhere).
- The list of added people is a fork setting in the local calendar accounts, beside Google, CalDAV and ICS. People are added from the directory picker in Settings > Calendar accounts.
- These events are read-only: no RSVP, no delete, no reminders.
- Each person gets the next palette colour; a right-click on the person in the calendar list picks another colour or removes them. The colour is stored with the local list.

## Consequences

- Deviation from "settings Teams knows are saved where Teams saves them": the people list is not shared with Teams or Outlook, because Graph has no write path for it.
- No event details beyond subject, location and time; opening a person's meeting shows what free/busy returns.
- A colleague's meeting that the user also attends shows twice, once per calendar, as in Outlook's overlay.
