---
status: proposed
date: 2026-11-27
superseded-by:
---

# ADR 0024: Google OAuth client in Settings

## Context

ADR 0009 compiles one Desktop client into release builds so Connect Google works from a Dock or Start Menu launch. That still leaves anyone who wants their own Google Cloud client rebuilding the app or exporting shell variables. A settings field was rejected in 0009 because it would have been the only path.

## Decision

Settings › Calendar accounts accepts a client id and client secret, with a short guide for a Desktop client. The pair is used for Connect and token refresh only when both are saved. The id is store metadata. The secret is in the OS keyring under `rusty-teams.google-oauth`, never in the calendar account document, never in logs, and never put back into the field. Clearing the id removes the pair.

Empty settings keep ADR 0009: runtime environment, then the compiled client. Release builds stay unchanged.

## Consequences

- A person can use their own Google Cloud project without a rebuild.
- Official builds still connect with one click when the fields are empty.
- Tokens issued for the previous client do not refresh after a switch. The save notice says to connect again; the app does not disconnect on its own.
- The compiled client id remains visible in release binaries, as in ADR 0009.
