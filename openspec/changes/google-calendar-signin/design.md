## Context

Google token refresh returns HTTP 400 `invalid_grant` when the refresh token is expired or revoked. The calendar notice then shows Retry, which calls the same load. Connect Google already opens the authorize URL with `prompt=consent` and `access_type=offline`, then appends a new account.

## Goals / Non-Goals

**Goals:**

- Offer Sign in on a Google auth failure and start the existing connect flow.
- Replace an existing account when the signed-in email matches.

**Non-Goals:**

- A separate Google login window inside the app.
- Disconnecting the account before sign-in.

## Decisions

1. The load failure records that sign-in is required when the message is a Google token-refresh failure or the missing-client notice. Other failures stay on Retry.
2. The Sign in action calls the existing connect flow.
3. Saving a finished connect replaces the account with the same email and keeps its id, so home calendar and write targets still point at it.

## Risks / Trade-offs

- The matched email is the primary calendar summary, which is the address Connect already stores. A summary that is not the address will not match and will add an account, the same as Connect does today.

## Migration Plan

Rebuild and restart the local binary. No data migration.

## Open Questions

None.
