## ADDED Requirements

### Requirement: Meeting reminder card floats on Linux

On Linux a meeting reminder MUST open the meeting card as a floating window, including when a desktop notification service is connected. The desktop notification MUST still be published. The card MUST use the same floating window kind as the incoming-call card. Chat message toasts MUST NOT open a card while that service is connected.

#### Scenario: Reminder with a notification service

- **WHEN** a meeting reminder is due on Linux and a desktop notification service is connected
- **THEN** the meeting card opens as a floating window
- **AND** the desktop notification is still published

#### Scenario: Chat toast stays a notification

- **WHEN** a chat message toast is due and a desktop notification service is connected
- **THEN** no chat card window is opened
