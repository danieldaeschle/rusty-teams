## Context

The sign-in banner copy assumes a browser window is open. The watchdog only opens that window when a tab is on a Microsoft login host. A Teams tab with no usable token stays headless, so the banner has nothing to point at and no button.

## Goals / Non-Goals

**Goals:**

- Open the managed browser once when Teams reports that sign-in is required and the window is hidden.
- Put a Sign in action on that banner so a closed window can be opened again.

**Non-Goals:**

- Reopening the window automatically after the user closes it.
- Google calendar token refresh.

## Decisions

1. The app connection state, not the tab host, decides that sign-in is required.
2. An automatic open happens once per hidden session. The Sign in action may open the window again after it was closed. A visible window is left alone.
3. The action label uses the existing Sign in string. The live-updates Retry action is unchanged apart from using the existing Retry string.

## Risks / Trade-offs

- The visible window may land on the Teams chat if the browser session is still signed in. The user can complete sign-in there. The app no longer asks them to use a window that does not exist.

## Migration Plan

Rebuild and restart the local binary. No data migration.

## Open Questions

None.
