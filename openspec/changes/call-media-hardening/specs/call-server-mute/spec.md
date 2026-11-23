## ADDED Requirements

### Requirement: Organizer mute closes the local microphone

When the roster reports the local participant as server-muted, the client SHALL show the microphone as muted and SHALL stop sending microphone audio. It MUST NOT post another mute for that transition.

#### Scenario: A colleague mutes the local user

- **WHEN** the local microphone is open and the roster marks the local participant server-muted
- **THEN** the microphone control shows muted
- **AND** the outgoing microphone track is disabled
- **AND** the client does not send a new mute state for that roster update

### Requirement: The local user can clear an organizer mute

The next microphone toggle after an organizer mute SHALL unmute. A roster update that still reports the same server mute MUST NOT mute the local user again. A later server mute, after the flag has cleared, SHALL mute the local user again.

#### Scenario: Toggle after an organizer mute

- **WHEN** the local user was muted by the server and presses the microphone control
- **THEN** the client requests an unmuted endpoint state
- **AND** a following roster update that still says server-muted leaves the microphone open

#### Scenario: A new organizer mute applies again

- **WHEN** the server mute flag drops and a later roster update sets it again while the local microphone is open
- **THEN** the microphone control shows muted again
