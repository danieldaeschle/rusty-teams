## ADDED Requirements

### Requirement: Missing token opens the sign-in window

When Teams needs a sign-in and the managed browser window is not visible, the app SHALL open that window for Microsoft sign-in. The app MUST NOT open a second window while one is already visible for that session. Closing the window MUST NOT immediately open another one. The sign-in banner SHALL offer a Sign in action that opens the window when none is visible.

#### Scenario: Headless session with no token shows the browser

- **WHEN** Teams needs a sign-in and the managed browser is running headless
- **THEN** the managed browser window becomes visible for sign-in
- **AND** a second window is not opened while that one stays visible

#### Scenario: Sign in action opens a closed window

- **WHEN** the sign-in banner is showing and the managed browser window is not visible
- **AND** the user chooses Sign in
- **THEN** the managed browser window opens for sign-in

#### Scenario: Sign in does not duplicate a visible window

- **WHEN** the managed browser window is already visible for sign-in
- **AND** the user chooses Sign in on the banner
- **THEN** the app does not open another browser window
