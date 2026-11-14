---
status: accepted
date: 2026-11-13
---

# ADR 0011: Calendar events open as a Teams-like peek

## Context

Opening a calendar event used a labeled dialog that showed a short body preview and little of the invite roster. Teams desktop shows a compact peek with agenda, RSVP, and an enlarged Tracking view. A full-page event route would leave the week grid; keeping the old dialog would keep diverging from Teams for people who live in the calendar.

Options were: keep the labeled dialog, navigate to a dedicated event page, or a Teams-like peek overlay that can enlarge.

## Decision

Opening an event shows a compact peek over the calendar: time, series, location, invite summary, scrollable full agenda (fetched body, not only the truncated preview), and RSVP. Enlarge opens the Tracking layout (details and agenda left, attendees by response right). Google-sourced events may show calendar name, reminders, source link, and Open in Google / Email guests. Outlook/Teams keep Email organizer and optional RSVP comment.

Untrusted agenda HTML is sanitized before render (plain Google descriptions are escaped and http(s)-linkified; Graph/Teams HTML drops non-http(s)/mailto hrefs).

## Consequences

- Calendar UX matches Teams chrome without a second navigation stack for every event.
- List payloads stay light; the peek loads full body and attendees on open.
- Status stays `proposed` until the calendar-peek pull request merges; then set `status` to `accepted`.
