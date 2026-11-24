## Context

On Linux, startup publishes an opening sign-in state before the managed browser launch returns. If that launch attaches to a browser that is already running headless, the state is left on opening. The splash treats every non-idle browser phase as more important than a warm cache, so the shell never appears.

The Teams tab on this machine is already `https://teams.cloud.microsoft/` in a headless Chromium. The splash text says the login page is still loading. No login window is mapped.

## Goals / Non-Goals

**Goals:**

- End the opening splash once a headless managed browser is ready.
- Keep the awaiting splash while a visible login window is open.

**Non-Goals:**

- Opening a login window when the Teams tab is already on `teams.cloud.microsoft`.
- Refreshing a revoked Google calendar token.

## Decisions

1. A successful launch that is not a visible managed window publishes idle. Visible success still publishes awaiting sign-in. Failure still publishes failed.
2. The splash rules stay as they are. Idle already falls through to the warm-cache ready path.

## Risks / Trade-offs

- If the session has no token, the shell appears with the existing sign-in banner instead of a splash. That matches a warm start where the browser is already on Teams and is not a login page.

## Migration Plan

Rebuild and restart the local binary. No data migration.

## Open Questions

None.
