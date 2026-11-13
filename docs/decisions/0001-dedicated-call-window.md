---
status: accepted
date: 2026-11-11
---

# ADR 0001: Calls and meetings open in their own window

## Context

An earlier deviation kept the call inside the main window. Teams does not: a call or meeting gets its own window, and the chat window stays usable. On 2026-11-11 Daniel replaced that deviation. On 2026-11-12 he added the capture rule.

Options were: keep the in-app call pane, or follow Teams with a separate window.

## Decision

A call or meeting opens in its own window. The main window stays free for chats. Sharing a window minimizes the call window, as Teams does. Layout and gallery follow Teams.

Only the share bar and the share border stay out of captures. The call window, the mini window, and gallery windows show up in screenshots, as in Teams.

## Consequences

- Chat stays reachable during a call.
- Window placement, gallery paging, and the toolbar are specified against Teams, not against a custom in-app layout.
- Screen-share capture rules in ADR 0005 must not hide the call window itself.
