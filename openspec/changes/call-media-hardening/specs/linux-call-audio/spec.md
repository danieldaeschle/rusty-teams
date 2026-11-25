## ADDED Requirements

### Requirement: Only Low uses the classical noise-suppression track

On the device track, Low SHALL publish the track with classical noise suppression. Auto, Off, and music mode SHALL publish the track without that stage. A failed software-path start MUST NOT restore the classical noise-suppression track for Off or music mode.

#### Scenario: Off does not restore noise suppression

- **WHEN** noise suppression is Off or music mode is on and the software path cannot start
- **THEN** the restored device track is the track without classical noise suppression

### Requirement: A named microphone matches the capture name

A named input on the software path SHALL match a capture device when the names are equal ignoring ASCII case, or when either name contains the other. cpal names differ from the Pulse description. An empty target SHALL NOT match. An unmatched name SHALL fail the capture with an error or the existing notice and MUST NOT silently capture a different microphone.

#### Scenario: A Pulse description matches a shorter capture name

- **WHEN** the listed name is "Jabra Speak 750 Mono" and cpal reports "Jabra Speak 750"
- **THEN** that capture device is selected

#### Scenario: Endpoint id is not a capture name

- **WHEN** the software path is asked for a named input whose key is an endpoint id and whose listed name is absent from the capture devices
- **THEN** capture fails with an error or notice

### Requirement: Computer sound falls back to the default microphone

While sharing computer sound, a failed named microphone SHALL fall back to the default microphone and show that notice. Silence is used only when the default microphone also fails.

#### Scenario: The default microphone opens

- **WHEN** the named microphone fails and the default microphone starts
- **THEN** the call uses the default microphone and shows the fallback notice

### Requirement: A device switch counts when the index is selected

Selecting an input or output SHALL be reported as applied when the device index is selected, even if recording or playout does not start on that attempt. A single failed start MUST NOT leave the previous device marked as current.

#### Scenario: Start fails once

- **WHEN** the device index is accepted and recording or playout does not start
- **THEN** the selection is still reported as applied
