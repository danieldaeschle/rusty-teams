# Proposal

## Why

When Google Calendar cannot load because the saved token was revoked, the calendar notice only offers Retry. Retry fetches again with the same token, so the user has no way to sign in.

## What Changes

- A Google load failure that needs sign-in shows Sign in instead of Retry.
- Sign in opens the existing Google Calendar authorize flow in the preferred browser.
- Completing sign-in for an email that is already connected replaces that account's tokens. It does not add a second copy.
- Other source failures keep Retry.
- Out of scope: changing Google OAuth scopes or the Teams sign-in window.

## Capabilities

### New Capabilities

- None.

### Modified Capabilities

- `calendar-accounts`: a Google calendar that failed for lack of a usable token offers Sign in, and a completed sign-in for an existing email replaces that account.

## Impact

- Calendar load notice action and Google account connect.
- No new surfaces, colours, or spacing.
