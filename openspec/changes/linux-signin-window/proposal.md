# Proposal

## Why

The sign-in banner tells the user to sign in in the browser window that opened. When Teams has no usable token but the managed browser stays headless on the Teams page, no window opens and the banner has no action. The user cannot sign in.

## What Changes

- When Teams needs a sign-in and the managed browser window is not visible, the app opens that window once.
- The sign-in banner shows a Sign in action. It opens the window when none is visible, and does not open a second window while one is already showing.
- Out of scope: Google calendar reconnect, and changing the Microsoft login page itself.

## Capabilities

### New Capabilities

- None.

### Modified Capabilities

- `linux-sign-in`: a missing Teams token must surface the managed browser window, and the sign-in banner must offer Sign in.

## Impact

- Linux managed-browser startup and the connection banner action.
- No new surfaces, colours, or spacing.
