---
status: accepted
date: 2026-11-12
---

# ADR 0007: Local agent CLIs sit beside Copilot

## Context

Microsoft Copilot is the Teams assistant, and `GUARDRAILS.md` forbids a fake Copilot UI for a feature we do not build. People on this client also run local CLIs (Claude Code, Copilot CLI, Cursor, and others). Hiding those behind Copilot branding, or letting them send into a chat on their own, both break the rule that other people must not see a non-Teams client.

Options were: Copilot only, local CLIs that send directly, or local CLIs that propose and keep Copilot as the default.

## Decision

The app discovers installed CLIs, including binaries under user directories that a desktop session often omits from PATH, and stores profiles and actions locally.

Empty "Write with" means Microsoft Copilot, with Copilot branding. A chosen chat profile answers the chat-header island and brands the rewrite control with that profile's name. The island prompt is the newest twenty non-deleted messages plus the question. That ask does not call Copilot.

MCP can propose a message, a reaction, or a meeting. Review before send stays on unless the user turns it off or grants a time window. Meeting proposals survive a restart. Nothing uses a custom message format on the server.

## Consequences

- Copilot remains the default and is not removed.
- A local writer is visible as itself, not as Copilot.
- Agents do not send until a human accepts, except where the user explicitly disabled review.
