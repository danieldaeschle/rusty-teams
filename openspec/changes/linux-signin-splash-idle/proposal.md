# Proposal

## Why

Restarting the app while the managed browser is already running headless leaves the splash on "Opening the browser for sign-in" forever. The browser is not opening, and a warm cache never appears.

## What Changes

- When the managed browser is already running in the background, the opening splash ends and the shell shows the cached session.
- A visible login window still keeps the awaiting-sign-in splash until login finishes or fails.
- Out of scope: Google calendar token refresh, and forcing a login window when the Teams tab is already open.

## Capabilities

### New Capabilities

- None.

### Modified Capabilities

- `linux-sign-in`: the opening splash must end once the managed browser is running headless, so a warm cache is not covered by that splash.

## Impact

- Linux managed-browser startup and the startup splash.
- No new surfaces, colours, or spacing.
