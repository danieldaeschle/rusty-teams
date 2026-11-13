---
status: accepted
date: 2026-11-12
---

# ADR 0008: Product UI is English and German

## Context

`GUARDRAILS.md` says Teams strings come from the Teams bundle, in the account language. The app's own chrome (settings, share notices, agent labels, calendar controls) is not in that bundle. Shipping it in one language leaves the other half of the users in the wrong language. Translating message bodies would corrupt other people's content.

Options were: English-only chrome, German-only chrome, or both, with the OS locale as the default.

## Decision

Product UI strings exist in English and German. First launch follows the OS locale when that language is one of the two, otherwise English. The user can switch in the app, the choice is stored, and it applies without a restart. A missing translation falls back to English.

Message bodies, names, and other people's content stay as they are. New chrome strings go into the locale catalog, not into the Teams bundle.

## Consequences

- Teams account language and app chrome language can differ. That is intentional.
- A new string is not done until the catalog has an English entry. German may fall back.
