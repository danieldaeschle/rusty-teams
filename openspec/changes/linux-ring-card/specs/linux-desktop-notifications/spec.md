# Spec Delta

## ADDED Requirements

### Requirement: Incoming ring shows the in-app ring card

When a desktop notification service is connected, an incoming ring MUST still open the in-app ring card. The card MUST show the caller and MUST offer decline and audio accept. When the ring is video, the card MUST also offer video accept. The desktop notification for that ring MUST still be published. The card MUST close when the ring ends. Chat toasts and meeting reminders MUST stay desktop notifications and MUST NOT open an in-app window.

#### Scenario: Ring while a notification service is connected

- **WHEN** an incoming call rings and a desktop notification service is connected
- **THEN** the in-app ring card is visible and the desktop notification for that ring is also shown

#### Scenario: Video ring offers video on the card

- **WHEN** the incoming ring is a video call
- **THEN** the ring card offers video accept as well as decline and audio accept

#### Scenario: Ring ends

- **WHEN** the ring is accepted, declined, or otherwise ends
- **THEN** the ring card closes

#### Scenario: Chat toast stays a notification

- **WHEN** a chat message toast is due and a desktop notification service is connected
- **THEN** the message is a desktop notification and no in-app toast window opens

#### Scenario: Meeting reminder stays a notification

- **WHEN** a meeting reminder is due and a desktop notification service is connected
- **THEN** the reminder is a desktop notification and no in-app toast window opens

### Requirement: Ring card actions answer the same ring

Decline, audio accept, and video accept on the ring card MUST act on the same ring as the matching desktop notification actions. Audio accept MUST answer without sending camera. Video accept MUST answer with the camera.

#### Scenario: Audio accept on the card

- **WHEN** the user accepts audio on the ring card
- **THEN** the call is answered without sending the camera and the desktop notification for that ring closes

#### Scenario: Decline on the card

- **WHEN** the user declines on the ring card
- **THEN** the ring is declined and the desktop notification for that ring closes
