## ADDED Requirements

### Requirement: Sign in from a failed Google calendar

When a connected Google calendar fails to load because the account needs sign-in, the calendar notice SHALL offer Sign in. Choosing Sign in MUST open the Google Calendar authorize flow in the user's preferred external browser, the same flow as Connect Google. Completing that flow for an email that is already connected MUST replace that account's tokens and MUST NOT add a second account for the same email. A failure that is not a sign-in problem MUST keep Retry.

#### Scenario: Revoked Google token offers Sign in

- **WHEN** Google Calendar fails to load because the saved token cannot be refreshed
- **THEN** the calendar notice offers Sign in
- **AND** does not offer Retry for that notice

#### Scenario: Sign in opens the browser

- **WHEN** the user chooses Sign in on that notice
- **THEN** the Google Calendar authorize page opens in the preferred external browser

#### Scenario: Same email replaces the account

- **WHEN** the user finishes Google sign-in and the email matches a connected Google account
- **THEN** that account's tokens are replaced
- **AND** the calendar list does not gain a second account for that email

#### Scenario: Other failures keep Retry

- **WHEN** Outlook or another source fails for a reason other than Google sign-in
- **THEN** the notice offers Retry
