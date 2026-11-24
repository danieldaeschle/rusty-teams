## ADDED Requirements

### Requirement: Headless browser does not hold the opening splash

When the managed browser is already running in the background, the app SHALL leave the opening-browser splash. Cached chats and channels MUST become visible. The opening splash SHALL stay only while the browser is actually opening, while a login window is visible, while sign-in is finishing, or when the browser failed to start.

#### Scenario: Existing headless session shows the cache

- **WHEN** the user starts the app and the managed browser is already running headless with the Teams profile
- **THEN** the opening-browser splash ends
- **AND** cached chats and channels are shown

#### Scenario: Visible login window still blocks the shell

- **WHEN** the managed browser window is visible and waiting for Microsoft sign-in
- **THEN** the splash stays on the awaiting-sign-in phase until that window finishes or the start fails
